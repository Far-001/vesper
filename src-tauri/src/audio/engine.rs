use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig, SupportedStreamConfigRange};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::decode::{open_source, PcmSource};
use super::eq::EqChain;
use super::spectrum::SpectrumMeter;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}

fn now_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1)
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    pub track_id: i64,
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
    pub bpm: f32,
    pub energy: f32,
    pub centroid: f32,
    pub reason: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackDto {
    pub track_id: Option<i64>,
    pub position: f64,
    pub duration: f64,
    pub playing: bool,
    pub volume: f32,
    pub repeat_mode: String,
    pub shuffle: bool,
    pub radio: bool,
    pub queue_index: usize,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub bpm: f32,
    pub energy: f32,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueEvt {
    pub items: Vec<QueueItem>,
    pub index: usize,
    pub radio: bool,
}

#[derive(Clone, Serialize)]
pub struct ErrorEvt {
    pub message: String,
}

struct Queue {
    items: Vec<QueueItem>,
    index: usize,
    radio: bool,
    shuffle: bool,
}

impl Queue {
    fn new() -> Self {
        Self {
            items: Vec::new(),
            index: 0,
            radio: false,
            shuffle: false,
        }
    }
}

struct SpecRing {
    data: Vec<f32>,
    pos: usize,
}

impl SpecRing {
    fn new() -> Self {
        Self {
            data: vec![0.0; 8192],
            pos: 0,
        }
    }

    fn push(&mut self, sample: f32) {
        self.data[self.pos] = sample;
        self.pos = (self.pos + 1) % self.data.len();
    }

    fn snapshot(&self, count: usize) -> Vec<f32> {
        let count = count.min(self.data.len());
        let mut out = vec![0.0; count];
        let start = (self.pos + self.data.len() - count) % self.data.len();
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = self.data[(start + i) % self.data.len()];
        }
        out
    }
}

struct Shared {
    buf: Mutex<VecDeque<f32>>,
    cap_samples: usize,
    device_rate: u32,
    device_channels: u16,
    paused: AtomicBool,
    stop: AtomicBool,
    active: AtomicBool,
    volume: AtomicU32,
    played_frames: AtomicU64,
    origin_frames: AtomicU64,
    duration_ms: AtomicU64,
    current_id: AtomicI64,
    repeat: AtomicU8,
    eq_gains: Mutex<[f32; 10]>,
    spec: Mutex<SpecRing>,
}

enum Cmd {
    Load(QueueItem),
    Seek(f64),
    Stop,
}

struct StereoResampler {
    in_rate: u32,
    out_rate: u32,
    hist_l: Vec<f32>,
    hist_r: Vec<f32>,
    pos: f64,
}

impl StereoResampler {
    fn new(rate: u32) -> Self {
        Self {
            in_rate: rate,
            out_rate: rate,
            hist_l: Vec::new(),
            hist_r: Vec::new(),
            pos: 0.0,
        }
    }

    fn reset(&mut self, in_rate: u32, out_rate: u32) {
        self.in_rate = in_rate.max(1);
        self.out_rate = out_rate.max(1);
        self.hist_l.clear();
        self.hist_r.clear();
        self.pos = 0.0;
    }

    fn process(&mut self, interleaved: &[f32], out: &mut Vec<f32>) {
        if self.in_rate == self.out_rate {
            out.extend_from_slice(interleaved);
            return;
        }
        for frame in interleaved.chunks_exact(2) {
            self.hist_l.push(frame[0]);
            self.hist_r.push(frame[1]);
        }
        let step = self.in_rate as f64 / self.out_rate as f64;
        while self.pos + 1.0 < self.hist_l.len() as f64 {
            let i0 = self.pos.floor() as usize;
            let t = (self.pos - i0 as f64) as f32;
            let i1 = i0 + 1;
            out.push(self.hist_l[i0] * (1.0 - t) + self.hist_l[i1] * t);
            out.push(self.hist_r[i0] * (1.0 - t) + self.hist_r[i1] * t);
            self.pos += step;
        }
        let drop_n = self.pos.floor() as usize;
        if drop_n > 0 && drop_n < self.hist_l.len() {
            self.hist_l.drain(..drop_n);
            self.hist_r.drain(..drop_n);
            self.pos -= drop_n as f64;
        }
    }
}

/// cpal marks `Stream` as non-Send on every platform so the type matches Android AAudio.
/// This player uses ALSA: the stream is started once and only dropped with the player.
struct AudioOut(cpal::Stream);

unsafe impl Send for AudioOut {}

pub struct Player {
    cmd_tx: Sender<Cmd>,
    shared: Arc<Shared>,
    queue: Arc<Mutex<Queue>>,
    app: AppHandle,
    stream: Mutex<AudioOut>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

impl Player {
    pub fn start(app: AppHandle, volume: f32, eq: [f32; 10]) -> Result<Self, String> {
        let (rate, channels, sample_format, config) = output_config()?;
        eprintln!("Vesper audio: {sample_format:?} {channels}ch {rate} Hz");
        let shared = Arc::new(Shared {
            buf: Mutex::new(VecDeque::with_capacity(rate as usize)),
            cap_samples: rate as usize * 2 * 4,
            device_rate: rate,
            device_channels: channels,
            paused: AtomicBool::new(true),
            stop: AtomicBool::new(false),
            active: AtomicBool::new(false),
            volume: AtomicU32::new(volume.clamp(0.0, 1.0).to_bits()),
            played_frames: AtomicU64::new(0),
            origin_frames: AtomicU64::new(0),
            duration_ms: AtomicU64::new(0),
            current_id: AtomicI64::new(-1),
            repeat: AtomicU8::new(0),
            eq_gains: Mutex::new(eq),
            spec: Mutex::new(SpecRing::new()),
        });
        let stream = build_stream(shared.clone(), &config, sample_format, channels)?;
        let queue = Arc::new(Mutex::new(Queue::new()));
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let mut threads = Vec::new();
        {
            let shared = shared.clone();
            let queue = queue.clone();
            let app = app.clone();
            threads.push(thread::spawn(move || decoder_loop(cmd_rx, shared, queue, app)));
        }
        {
            let shared = shared.clone();
            let app = app.clone();
            threads.push(thread::spawn(move || spectrum_loop(shared, app)));
        }
        {
            let shared = shared.clone();
            let queue = queue.clone();
            let app = app.clone();
            threads.push(thread::spawn(move || playback_loop(shared, queue, app)));
        }
        Ok(Self {
            cmd_tx,
            shared,
            queue,
            app,
            stream: Mutex::new(AudioOut(stream)),
            threads: Mutex::new(threads),
        })
    }

    pub fn load_queue(&self, mut items: Vec<QueueItem>, mut index: usize, radio: bool) {
        if items.is_empty() {
            return;
        }
        index = index.min(items.len() - 1);
        let shuffle = lock(&self.queue).shuffle;
        if shuffle && !radio && items.len() > 1 {
            let selected = items.remove(index);
            shuffle_slice(&mut items, now_seed());
            items.insert(0, selected);
            index = 0;
        }
        let item = items[index].clone();
        {
            let mut queue = lock(&self.queue);
            queue.items = items;
            queue.index = index;
            queue.radio = radio;
        }
        let _ = self.cmd_tx.send(Cmd::Load(item));
        self.emit_queue();
    }

    pub fn play_index(&self, index: usize) {
        let item = {
            let mut queue = lock(&self.queue);
            if index >= queue.items.len() {
                return;
            }
            queue.index = index;
            queue.items[index].clone()
        };
        let _ = self.cmd_tx.send(Cmd::Load(item));
        self.emit_queue();
    }

    pub fn pause(&self) {
        self.shared.paused.store(true, Ordering::Relaxed);
        self.emit_playback();
    }

    pub fn resume(&self) {
        if self.shared.active.load(Ordering::Relaxed) {
            self.shared.paused.store(false, Ordering::Relaxed);
            self.emit_playback();
        }
    }

    pub fn toggle(&self) {
        let paused = self.shared.paused.load(Ordering::Relaxed);
        let active = self.shared.active.load(Ordering::Relaxed);
        if paused || !active {
            self.resume();
        } else {
            self.pause();
        }
    }

    pub fn seek(&self, secs: f64) {
        let _ = self.cmd_tx.send(Cmd::Seek(secs.max(0.0)));
    }

    pub fn next(&self) {
        let item = {
            let mut queue = lock(&self.queue);
            if queue.items.is_empty() {
                return;
            }
            if queue.index + 1 < queue.items.len() {
                queue.index += 1;
            } else if self.shared.repeat.load(Ordering::Relaxed) == 1 {
                queue.index = 0;
            } else {
                return;
            }
            queue.items[queue.index].clone()
        };
        let _ = self.cmd_tx.send(Cmd::Load(item));
        self.emit_queue();
    }

    pub fn prev(&self) {
        if self.position_secs() > 3.0 {
            self.seek(0.0);
            return;
        }
        let item = {
            let mut queue = lock(&self.queue);
            if queue.items.is_empty() {
                return;
            }
            if queue.index > 0 {
                queue.index -= 1;
                queue.items[queue.index].clone()
            } else {
                drop(queue);
                self.seek(0.0);
                return;
            }
        };
        let _ = self.cmd_tx.send(Cmd::Load(item));
        self.emit_queue();
    }

    pub fn set_volume(&self, volume: f32) {
        self.shared
            .volume
            .store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        self.emit_playback();
    }

    pub fn volume(&self) -> f32 {
        f32::from_bits(self.shared.volume.load(Ordering::Relaxed))
    }

    pub fn set_eq(&self, gains: [f32; 10]) {
        *lock(&self.shared.eq_gains) = gains;
    }

    pub fn eq(&self) -> [f32; 10] {
        *lock(&self.shared.eq_gains)
    }

    pub fn set_repeat(&self, mode: &str) {
        let value = match mode {
            "all" => 1,
            "one" => 2,
            _ => 0,
        };
        self.shared.repeat.store(value, Ordering::Relaxed);
        self.emit_playback();
    }

    pub fn set_shuffle(&self, on: bool) {
        {
            let mut queue = lock(&self.queue);
            queue.shuffle = on;
            let index = queue.index;
            if on && index + 1 < queue.items.len() {
                let tail = &mut queue.items[index + 1..];
                shuffle_slice(tail, now_seed());
            }
        }
        self.emit_queue();
        self.emit_playback();
    }

    pub fn current_track_id(&self) -> Option<i64> {
        let id = self.shared.current_id.load(Ordering::Relaxed);
        if id >= 0 {
            Some(id)
        } else {
            None
        }
    }

    pub fn snapshot(&self) -> PlaybackDto {
        snapshot(&self.shared, &self.queue)
    }

    pub fn queue_snapshot(&self) -> QueueEvt {
        let queue = lock(&self.queue);
        QueueEvt {
            items: queue.items.clone(),
            index: queue.index,
            radio: queue.radio,
        }
    }

    fn position_secs(&self) -> f64 {
        position_secs(&self.shared)
    }

    fn emit_playback(&self) {
        let _ = self.app.emit("playback", self.snapshot());
    }

    fn emit_queue(&self) {
        let _ = self.app.emit("queue", self.queue_snapshot());
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        let _ = self.cmd_tx.send(Cmd::Stop);
        for handle in lock(&self.threads).drain(..) {
            let _ = handle.join();
        }
        drop(lock(&self.stream));
    }
}

fn shuffle_slice(items: &mut [QueueItem], mut state: u64) {
    if items.len() < 2 {
        return;
    }
    state |= 1;
    for i in (1..items.len()).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let j = (state as usize) % (i + 1);
        items.swap(i, j);
    }
}

fn repeat_name(value: u8) -> &'static str {
    match value {
        1 => "all",
        2 => "one",
        _ => "off",
    }
}

fn position_secs(shared: &Shared) -> f64 {
    let played = shared.played_frames.load(Ordering::Relaxed) as f64;
    let origin = shared.origin_frames.load(Ordering::Relaxed) as f64;
    let buffered = lock(&shared.buf).len() as f64 / 2.0;
    let rate = shared.device_rate.max(1) as f64;
    ((origin + played - buffered) / rate).max(0.0)
}

fn snapshot(shared: &Shared, queue: &Mutex<Queue>) -> PlaybackDto {
    let (index, radio, shuffle, current) = {
        let queue = lock(queue);
        let current = queue.items.get(queue.index).cloned();
        (queue.index, queue.radio, queue.shuffle, current)
    };
    let id = shared.current_id.load(Ordering::Relaxed);
    let duration_ms = shared.duration_ms.load(Ordering::Relaxed) as f64 / 1000.0;
    PlaybackDto {
        track_id: if id >= 0 { Some(id) } else { None },
        position: position_secs(shared),
        duration: if duration_ms > 0.0 {
            duration_ms
        } else {
            current.as_ref().map(|item| item.duration).unwrap_or(0.0)
        },
        playing: shared.active.load(Ordering::Relaxed) && !shared.paused.load(Ordering::Relaxed),
        volume: f32::from_bits(shared.volume.load(Ordering::Relaxed)),
        repeat_mode: repeat_name(shared.repeat.load(Ordering::Relaxed)).to_string(),
        shuffle,
        radio,
        queue_index: index,
        title: current.as_ref().map(|item| item.title.clone()).unwrap_or_default(),
        artist: current.as_ref().map(|item| item.artist.clone()).unwrap_or_default(),
        album: current.as_ref().map(|item| item.album.clone()).unwrap_or_default(),
        bpm: current.as_ref().map(|item| item.bpm).unwrap_or(0.0),
        energy: current.as_ref().map(|item| item.energy).unwrap_or(0.0),
    }
}

fn reset_clock(shared: &Shared, origin_frames: u64) {
    let mut buf = lock(&shared.buf);
    buf.clear();
    shared.played_frames.store(0, Ordering::Relaxed);
    shared.origin_frames.store(origin_frames, Ordering::Relaxed);
}

fn emit_error(app: &AppHandle, message: impl Into<String>) {
    let _ = app.emit("error", ErrorEvt { message: message.into() });
}

fn decoder_loop(rx: Receiver<Cmd>, shared: Arc<Shared>, queue: Arc<Mutex<Queue>>, app: AppHandle) {
    let mut source: Option<Box<dyn PcmSource>> = None;
    let mut resampler = StereoResampler::new(shared.device_rate);
    let mut eq = EqChain::new(shared.device_rate);
    let mut pending: Vec<f32> = Vec::new();
    let mut skips = 0_u8;

    loop {
        if shared.stop.load(Ordering::Relaxed) {
            break;
        }
        let mut load: Option<QueueItem> = None;
        let mut seek: Option<f64> = None;
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                Cmd::Stop => return,
                Cmd::Load(item) => load = Some(item),
                Cmd::Seek(secs) => seek = Some(secs),
            }
        }
        if let Some(item) = load {
            match open_item(&item, &shared, &mut resampler, &mut eq, &mut pending) {
                Ok(opened) => {
                    source = Some(opened);
                    skips = 0;
                    let _ = app.emit("playback", snapshot(&shared, &queue));
                }
                Err(err) => {
                    emit_error(&app, err);
                    source = None;
                    shared.active.store(false, Ordering::Relaxed);
                    skips = skips.saturating_add(1);
                    if skips < 8 {
                        if let Some(next) = skip_forward(&queue, shared.repeat.load(Ordering::Relaxed)) {
                            let _ = app.emit("queue", queue_evt(&queue));
                            let _ = rx_try_load(&rx, next, &mut source, &shared, &mut resampler, &mut eq, &mut pending, &app, &queue, &mut skips);
                        }
                    }
                }
            }
        }
        if let Some(secs) = seek {
            if let Some(opened) = source.as_mut() {
                let duration = shared.duration_ms.load(Ordering::Relaxed) as f64 / 1000.0;
                let secs = if duration > 0.5 {
                    secs.min((duration - 0.1).max(0.0))
                } else {
                    secs
                };
                if opened.seek(secs).is_ok() {
                    resampler.reset(opened.sample_rate(), shared.device_rate);
                    eq.reset_state();
                    pending.clear();
                    let frame = (secs * shared.device_rate as f64) as u64;
                    reset_clock(&shared, frame);
                    let _ = app.emit("playback", snapshot(&shared, &queue));
                }
            }
        }

        eq.set_gains(*lock(&shared.eq_gains));
        flush_pending(&shared, &mut pending);

        if shared.paused.load(Ordering::Relaxed) || source.is_none() {
            thread::sleep(Duration::from_millis(12));
            continue;
        }
        if pending.len() > 16_384 {
            thread::sleep(Duration::from_millis(6));
            continue;
        }

        let read = source.as_mut().unwrap().read_stereo_frames(2048);
        match read {
            Ok(Some(frames)) => {
                let mut processed = Vec::new();
                resampler.process(&frames, &mut processed);
                eq.process_interleaved(&mut processed);
                pending.extend(processed);
                flush_pending(&shared, &mut pending);
            }
            Ok(None) => {
                if !pending.is_empty() || lock(&shared.buf).len() > 4 {
                    thread::sleep(Duration::from_millis(8));
                    continue;
                }
                match after_eof(&queue, shared.repeat.load(Ordering::Relaxed)) {
                    Eof::Replay => {
                        if let Some(opened) = source.as_mut() {
                            let _ = opened.seek(0.0);
                            resampler.reset(opened.sample_rate(), shared.device_rate);
                            eq.reset_state();
                            pending.clear();
                            reset_clock(&shared, 0);
                        }
                    }
                    Eof::Load(item) => {
                        let _ = app.emit("queue", queue_evt(&queue));
                        match open_item(&item, &shared, &mut resampler, &mut eq, &mut pending) {
                            Ok(opened) => {
                                source = Some(opened);
                                let _ = app.emit("playback", snapshot(&shared, &queue));
                            }
                            Err(err) => {
                                emit_error(&app, err);
                                source = None;
                                shared.active.store(false, Ordering::Relaxed);
                            }
                        }
                    }
                    Eof::Stop => {
                        source = None;
                        pending.clear();
                        shared.active.store(false, Ordering::Relaxed);
                        shared.paused.store(true, Ordering::Relaxed);
                        reset_clock(&shared, 0);
                        let _ = app.emit("playback", snapshot(&shared, &queue));
                    }
                }
            }
            Err(err) => {
                emit_error(&app, err);
                source = None;
                shared.active.store(false, Ordering::Relaxed);
            }
        }
    }
}

fn rx_try_load(
    _rx: &Receiver<Cmd>,
    item: QueueItem,
    source: &mut Option<Box<dyn PcmSource>>,
    shared: &Shared,
    resampler: &mut StereoResampler,
    eq: &mut EqChain,
    pending: &mut Vec<f32>,
    app: &AppHandle,
    queue: &Mutex<Queue>,
    skips: &mut u8,
) {
    match open_item(&item, shared, resampler, eq, pending) {
        Ok(opened) => {
            *source = Some(opened);
            *skips = 0;
            let _ = app.emit("playback", snapshot(shared, queue));
        }
        Err(err) => {
            emit_error(app, err);
            *source = None;
        }
    }
}

fn open_item(
    item: &QueueItem,
    shared: &Shared,
    resampler: &mut StereoResampler,
    eq: &mut EqChain,
    pending: &mut Vec<f32>,
) -> Result<Box<dyn PcmSource>, String> {
    let source = open_source(Path::new(&item.path), shared.device_rate)?;
    resampler.reset(source.sample_rate(), shared.device_rate);
    eq.set_sample_rate(shared.device_rate);
    eq.reset_state();
    pending.clear();
    reset_clock(shared, 0);
    shared
        .duration_ms
        .store((item.duration.max(0.0) * 1000.0) as u64, Ordering::Relaxed);
    shared.current_id.store(item.track_id, Ordering::Relaxed);
    shared.active.store(true, Ordering::Relaxed);
    shared.paused.store(false, Ordering::Relaxed);
    Ok(source)
}

fn flush_pending(shared: &Shared, pending: &mut Vec<f32>) {
    if pending.len() < 2 {
        return;
    }
    let mut buf = lock(&shared.buf);
    let space = shared.cap_samples.saturating_sub(buf.len()) & !1;
    let count = space.min(pending.len()) & !1;
    if count == 0 {
        return;
    }
    buf.extend(pending.drain(..count));
}

enum Eof {
    Replay,
    Load(QueueItem),
    Stop,
}

fn after_eof(queue: &Mutex<Queue>, repeat: u8) -> Eof {
    if repeat == 2 {
        return Eof::Replay;
    }
    let mut queue = lock(queue);
    if queue.items.is_empty() {
        return Eof::Stop;
    }
    if queue.index + 1 < queue.items.len() {
        queue.index += 1;
        return Eof::Load(queue.items[queue.index].clone());
    }
    if repeat == 1 {
        queue.index = 0;
        return Eof::Load(queue.items[0].clone());
    }
    Eof::Stop
}

fn skip_forward(queue: &Mutex<Queue>, repeat: u8) -> Option<QueueItem> {
    match after_eof(queue, if repeat == 2 { 1 } else { repeat }) {
        Eof::Load(item) => Some(item),
        _ => None,
    }
}

fn queue_evt(queue: &Mutex<Queue>) -> QueueEvt {
    let queue = lock(queue);
    QueueEvt {
        items: queue.items.clone(),
        index: queue.index,
        radio: queue.radio,
    }
}

fn spectrum_loop(shared: Arc<Shared>, app: AppHandle) {
    let mut meter = SpectrumMeter::new(shared.device_rate);
    while !shared.stop.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(33));
        let samples = lock(&shared.spec).snapshot(2048);
        let bands = meter.bands(&samples);
        let _ = app.emit(
            "spectrum",
            SpectrumEvt {
                bands: bands.to_vec(),
            },
        );
    }
}

fn playback_loop(shared: Arc<Shared>, queue: Arc<Mutex<Queue>>, app: AppHandle) {
    while !shared.stop.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(100));
        let _ = app.emit("playback", snapshot(&shared, &queue));
    }
}

#[derive(Clone, Serialize)]
struct SpectrumEvt {
    bands: Vec<f32>,
}

fn report_stream_error(err: cpal::StreamError) {
    eprintln!("audio stream: {err}");
}

fn render(shared: &Shared, data: &mut [f32], channels: usize) {
    let channels = channels.max(1);
    let volume = f32::from_bits(shared.volume.load(Ordering::Relaxed)).clamp(0.0, 1.0);
    let silent = shared.paused.load(Ordering::Relaxed) || !shared.active.load(Ordering::Relaxed);
    let mut buf = lock(&shared.buf);
    let mut spec = lock(&shared.spec);
    let frames = data.len() / channels;
    for frame in 0..frames {
        let (left, right) = if silent || buf.len() < 2 {
            (0.0, 0.0)
        } else {
            let left = buf.pop_front().unwrap_or(0.0);
            let right = buf.pop_front().unwrap_or(0.0);
            shared.played_frames.fetch_add(1, Ordering::Relaxed);
            (left * volume, right * volume)
        };
        let base = frame * channels;
        if channels == 1 {
            data[base] = 0.5 * (left + right);
        } else {
            data[base] = left;
            data[base + 1] = right;
            for channel in 2..channels {
                data[base + channel] = 0.0;
            }
        }
        spec.push(0.5 * (left + right));
    }
}

fn output_config() -> Result<(u32, u16, SampleFormat, StreamConfig), String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "Нет устройства вывода".to_string())?;
    let mut ranges = device
        .supported_output_configs()
        .map_err(|err| err.to_string())?
        .collect::<Vec<_>>();
    let range = ranges
        .iter()
        .find(|range| range.channels() == 2 && range.sample_format() == SampleFormat::F32)
        .or_else(|| ranges.iter().find(|range| range.sample_format() == SampleFormat::F32))
        .cloned()
        .or_else(|| {
            device
                .default_output_config()
                .ok()
                .and_then(|config| {
                    ranges.iter().find(|range| {
                        range.channels() == config.channels() && range.sample_format() == config.sample_format()
                    }).cloned()
                })
        })
        .ok_or_else(|| "Устройство вывода не сообщает формат".to_string())?;
    let rate = pick_rate(&range);
    let sample_format = range.sample_format();
    let channels = range.channels();
    let supported = range.with_sample_rate(cpal::SampleRate(rate));
    Ok((rate, channels, sample_format, supported.into()))
}

fn pick_rate(range: &SupportedStreamConfigRange) -> u32 {
    let min = range.min_sample_rate().0;
    let max = range.max_sample_rate().0;
    48_000.clamp(min, max)
}

fn build_stream(
    shared: Arc<Shared>,
    config: &StreamConfig,
    sample_format: SampleFormat,
    channels: u16,
) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "Нет устройства вывода".to_string())?;
    let channel_count = channels as usize;
    let stream = match sample_format {
        SampleFormat::F32 => {
            let shared = shared.clone();
            device.build_output_stream(
                config,
                move |data: &mut [f32], _| render(&shared, data, channel_count),
                report_stream_error,
                None,
            )
        }
        SampleFormat::I16 => {
            let shared = shared.clone();
            let mut scratch = Vec::<f32>::new();
            device.build_output_stream(
                config,
                move |data: &mut [i16], _| {
                    if scratch.len() < data.len() {
                        scratch.resize(data.len(), 0.0);
                    }
                    render(&shared, &mut scratch[..data.len()], channel_count);
                    for (dst, src) in data.iter_mut().zip(scratch.iter()) {
                        *dst = (src.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                    }
                },
                report_stream_error,
                None,
            )
        }
        SampleFormat::U16 => {
            let shared = shared.clone();
            let mut scratch = Vec::<f32>::new();
            device.build_output_stream(
                config,
                move |data: &mut [u16], _| {
                    if scratch.len() < data.len() {
                        scratch.resize(data.len(), 0.0);
                    }
                    render(&shared, &mut scratch[..data.len()], channel_count);
                    for (dst, src) in data.iter_mut().zip(scratch.iter()) {
                        let unit = (src.clamp(-1.0, 1.0) + 1.0) * 0.5;
                        *dst = (unit * u16::MAX as f32) as u16;
                    }
                },
                report_stream_error,
                None,
            )
        }
        other => return Err(format!("формат вывода {other:?} не поддерживается")),
    }
    .map_err(|err| err.to_string())?;
    stream.play().map_err(|err| err.to_string())?;
    Ok(stream)
}
