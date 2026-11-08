use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder, DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

pub trait PcmSource: Send {
    fn sample_rate(&self) -> u32;
    fn read_stereo_frames(&mut self, max_frames: usize) -> Result<Option<Vec<f32>>, String>;
    fn seek(&mut self, secs: f64) -> Result<(), String>;
}

pub fn open_source(path: &Path, ffmpeg_rate: u32) -> Result<Box<dyn PcmSource>, String> {
    match SymphoniaSource::open(path) {
        Ok(source) => Ok(Box::new(source)),
        Err(sym_err) => {
            if ffmpeg_available() {
                match FfmpegSource::open(path, ffmpeg_rate) {
                    Ok(source) => Ok(Box::new(source)),
                    Err(ff_err) => Err(format!("{sym_err}; ffmpeg: {ff_err}")),
                }
            } else {
                Err(format!(
                    "{sym_err}. Для редких форматов нужен ffmpeg в PATH."
                ))
            }
        }
    }
}

pub fn decode_mono_prefix(path: &Path, max_secs: f64) -> Result<(Vec<f32>, u32), String> {
    let mut source = open_source(path, 22_050)?;
    let rate = source.sample_rate();
    let max_frames = (max_secs * rate as f64).max(0.0) as usize;
    let mut stereo = Vec::new();
    while stereo.len() / 2 < max_frames {
        let need = (max_frames - stereo.len() / 2).clamp(1, 4096);
        match source.read_stereo_frames(need)? {
            Some(chunk) => stereo.extend(chunk),
            None => break,
        }
    }
    let mut mono = Vec::with_capacity(stereo.len() / 2);
    for frame in stereo.chunks_exact(2) {
        mono.push(0.5 * (frame[0] + frame[1]));
    }
    let mono = resample_mono(&mono, rate, 22_050);
    Ok((mono, 22_050))
}

fn resample_mono(input: &[f32], in_rate: u32, out_rate: u32) -> Vec<f32> {
    if input.is_empty() || in_rate == 0 || in_rate == out_rate {
        return input.to_vec();
    }
    let ratio = in_rate as f64 / out_rate as f64;
    let out_len = ((input.len() as f64) / ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let pos = i as f64 * ratio;
        let i0 = pos.floor() as usize;
        let i1 = (i0 + 1).min(input.len() - 1);
        let t = (pos - i0 as f64) as f32;
        out.push(input[i0] * (1.0 - t) + input[i1] * t);
    }
    out
}

fn ffmpeg_available() -> bool {
    static OK: OnceLock<bool> = OnceLock::new();
    *OK.get_or_init(|| {
        Command::new("ffmpeg")
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    })
}

fn to_stereo(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        let mut out = Vec::with_capacity(interleaved.len() * 2);
        for sample in interleaved {
            out.push(*sample);
            out.push(*sample);
        }
        return out;
    }
    if channels == 2 {
        return interleaved.to_vec();
    }
    let frames = interleaved.len() / channels;
    let mut out = Vec::with_capacity(frames * 2);
    for frame in 0..frames {
        out.push(interleaved[frame * channels]);
        out.push(interleaved[frame * channels + 1]);
    }
    out
}

struct SymphoniaSource {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    sample_rate: u32,
    channels: usize,
    buffer: Option<SampleBuffer<f32>>,
}

impl SymphoniaSource {
    fn open(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|err| format!("не открыть файл: {err}"))?;
        let source = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
            hint.with_extension(ext);
        }
        let probed = symphonia::default::get_probe()
            .format(&hint, source, &FormatOptions::default(), &MetadataOptions::default())
            .map_err(|err| format!("формат не поддерживается ({err})"))?;
        let mut format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
            .cloned()
            .ok_or_else(|| "в файле нет аудиодорожки".to_string())?;
        let sample_rate = track
            .codec_params
            .sample_rate
            .ok_or_else(|| "у дорожки нет частоты дискретизации".to_string())?;
        let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(2);
        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|err| format!("декодер: {err}"))?;
        Ok(Self {
            format,
            decoder,
            track_id: track.id,
            sample_rate,
            channels,
            buffer: None,
        })
    }
}

impl PcmSource for SymphoniaSource {
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn read_stereo_frames(&mut self, max_frames: usize) -> Result<Option<Vec<f32>>, String> {
        let mut out = Vec::new();
        while out.len() / 2 < max_frames {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(SymError::IoError(err)) if err.kind() == std::io::ErrorKind::UnexpectedEof => {
                    return if out.is_empty() { Ok(None) } else { Ok(Some(out)) };
                }
                Err(SymError::ResetRequired) => {
                    self.decoder.reset();
                    continue;
                }
                Err(err) => {
                    return if out.is_empty() {
                        Err(format!("чтение: {err}"))
                    } else {
                        Ok(Some(out))
                    };
                }
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            let decoded = match self.decoder.decode(&packet) {
                Ok(decoded) => decoded,
                Err(SymError::DecodeError(_)) => continue,
                Err(SymError::IoError(err)) if err.kind() == std::io::ErrorKind::UnexpectedEof => {
                    return if out.is_empty() { Ok(None) } else { Ok(Some(out)) };
                }
                Err(err) => return Err(format!("декод: {err}")),
            };
            let spec = *decoded.spec();
            let channels = spec.channels.count();
            let need_new = self
                .buffer
                .as_ref()
                .map(|buf| (buf.capacity() as usize) < decoded.capacity())
                .unwrap_or(true);
            if need_new {
                self.buffer = Some(SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
            }
            let buffer = self.buffer.as_mut().unwrap();
            buffer.copy_interleaved_ref(decoded);
            if spec.rate != 0 {
                self.sample_rate = spec.rate;
            }
            self.channels = channels;
            out.extend(to_stereo(buffer.samples(), channels));
        }
        if out.is_empty() {
            Ok(None)
        } else {
            Ok(Some(out))
        }
    }

    fn seek(&mut self, secs: f64) -> Result<(), String> {
        let secs = secs.max(0.0);
        let time = Time::new(secs.trunc() as u64, secs.fract());
        self.format
            .seek(
                SeekMode::Coarse,
                SeekTo::Time {
                    time,
                    track_id: Some(self.track_id),
                },
            )
            .map_err(|err| format!("перемотка: {err}"))?;
        self.decoder.reset();
        Ok(())
    }
}

struct FfmpegSource {
    path: PathBuf,
    rate: u32,
    child: Option<Child>,
    stdout: Option<std::process::ChildStdout>,
}

impl FfmpegSource {
    fn open(path: &Path, rate: u32) -> Result<Self, String> {
        let mut source = Self {
            path: path.to_path_buf(),
            rate,
            child: None,
            stdout: None,
        };
        source.spawn(0.0)?;
        Ok(source)
    }

    fn spawn(&mut self, seek_to: f64) -> Result<(), String> {
        self.stop_child();
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-v").arg("error");
        if seek_to > 0.05 {
            cmd.arg("-ss").arg(format!("{seek_to:.3}"));
        }
        cmd.arg("-i")
            .arg(&self.path)
            .arg("-f")
            .arg("f32le")
            .arg("-acodec")
            .arg("pcm_f32le")
            .arg("-ac")
            .arg("2")
            .arg("-ar")
            .arg(self.rate.to_string())
            .arg("pipe:1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd.spawn().map_err(|err| format!("запуск ffmpeg: {err}"))?;
        self.stdout = child.stdout.take();
        self.child = Some(child);
        Ok(())
    }

    fn stop_child(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.child = None;
        self.stdout = None;
    }
}

impl Drop for FfmpegSource {
    fn drop(&mut self) {
        self.stop_child();
    }
}

impl PcmSource for FfmpegSource {
    fn sample_rate(&self) -> u32 {
        self.rate
    }

    fn read_stereo_frames(&mut self, max_frames: usize) -> Result<Option<Vec<f32>>, String> {
        let stdout = self.stdout.as_mut().ok_or_else(|| "нет потока ffmpeg".to_string())?;
        let mut bytes = vec![0_u8; max_frames.saturating_mul(8).max(8)];
        let mut filled = 0;
        while filled < bytes.len() {
            match stdout.read(&mut bytes[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(err) => return Err(err.to_string()),
            }
        }
        let usable = filled - (filled % 8);
        if usable == 0 {
            return Ok(None);
        }
        let mut out = Vec::with_capacity(usable / 4);
        for chunk in bytes[..usable].chunks_exact(4) {
            out.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
        Ok(Some(out))
    }

    fn seek(&mut self, secs: f64) -> Result<(), String> {
        self.spawn(secs.max(0.0))
    }
}

#[cfg(test)]
pub(crate) fn write_wav_mono(path: &Path, samples: &[f32], rate: u32) -> std::io::Result<()> {
    let mut data = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        data.extend_from_slice(&value.to_le_bytes());
    }
    let data_len = data.len() as u32;
    let mut file = File::create(path)?;
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + data_len).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&rate.to_le_bytes())?;
    file.write_all(&(rate * 2).to_le_bytes())?;
    file.write_all(&2u16.to_le_bytes())?;
    file.write_all(&16u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_len.to_le_bytes())?;
    file.write_all(&data)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_roundtrip_keeps_energy() {
        let dir = std::env::temp_dir().join(format!("vesper-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tone.wav");
        let rate = 22_050;
        let mut samples = Vec::new();
        for i in 0..(rate as usize) {
            samples.push((2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin() * 0.5);
        }
        write_wav_mono(&path, &samples, rate).unwrap();
        let (mono, out_rate) = decode_mono_prefix(&path, 2.0).unwrap();
        assert_eq!(out_rate, 22_050);
        assert!(mono.len() > rate as usize / 2);
        let energy: f32 = mono.iter().map(|s| s * s).sum::<f32>() / mono.len() as f32;
        assert!(energy > 0.05, "energy {energy}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
