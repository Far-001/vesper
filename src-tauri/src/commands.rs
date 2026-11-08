use std::sync::MutexGuard;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use serde::Serialize;
use tauri::State;

use crate::audio::{preset, PlaybackDto, Player, QueueItem};
use crate::library::{
    self, cover_data_url, ensure_analyzed, get_setting, list_tracks, scan_root, set_setting, tracks_by_ids, TrackDto,
};
use crate::radio::{build_radio, Feat};
use crate::state::AppState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    pub tracks: Vec<TrackDto>,
    pub root: Option<String>,
    pub eq: Vec<f32>,
    pub volume: f32,
    pub playback: PlaybackDto,
    pub queue: Vec<QueueItem>,
    pub queue_index: usize,
    pub radio: bool,
    pub audio_error: Option<String>,
}

fn db(state: &AppState) -> MutexGuard<'_, Connection> {
    state.db.lock().unwrap_or_else(|err| err.into_inner())
}

fn player(state: &AppState) -> Result<&Player, String> {
    state
        .player
        .as_ref()
        .ok_or_else(|| state.audio_error.clone().unwrap_or_else(|| "Нет звука".into()))
}

fn empty_playback(volume: f32) -> PlaybackDto {
    PlaybackDto {
        track_id: None,
        position: 0.0,
        duration: 0.0,
        playing: false,
        volume,
        repeat_mode: "off".into(),
        shuffle: false,
        radio: false,
        queue_index: 0,
        title: String::new(),
        artist: String::new(),
        album: String::new(),
        bpm: 0.0,
        energy: 0.0,
    }
}

fn read_eq(conn: &Connection) -> [f32; 10] {
    let Some(raw) = get_setting(conn, "eq") else {
        return [0.0; 10];
    };
    let values: Vec<f32> = serde_json::from_str(&raw).unwrap_or_default();
    if values.len() != 10 {
        return [0.0; 10];
    }
    let mut bands = [0.0; 10];
    bands.copy_from_slice(&values);
    bands
}

fn read_volume(conn: &Connection) -> f32 {
    get_setting(conn, "volume")
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(0.85)
        .clamp(0.0, 1.0)
}

pub fn saved_volume(conn: &Connection) -> f32 {
    read_volume(conn)
}

pub fn saved_eq(conn: &Connection) -> [f32; 10] {
    read_eq(conn)
}

fn to_item(track: TrackDto, reason: String) -> QueueItem {
    QueueItem {
        track_id: track.id,
        path: track.path,
        title: track.title,
        artist: track.artist,
        album: track.album,
        duration: track.duration,
        bpm: track.bpm,
        energy: track.energy,
        centroid: track.centroid,
        reason,
    }
}

#[tauri::command]
pub fn bootstrap(state: State<'_, AppState>) -> Result<Bootstrap, String> {
    let conn = db(&state);
    let tracks = list_tracks(&conn, "")?;
    let root = get_setting(&conn, "root");
    let eq = read_eq(&conn);
    let volume = read_volume(&conn);
    drop(conn);
    let (playback, queue, queue_index, radio) = if let Some(player) = state.player.as_ref() {
        let playback = player.snapshot();
        let queue = player.queue_snapshot();
        (playback, queue.items, queue.index, queue.radio)
    } else {
        (empty_playback(volume), Vec::new(), 0, false)
    };
    Ok(Bootstrap {
        tracks,
        root,
        eq: eq.to_vec(),
        volume,
        playback,
        queue,
        queue_index,
        radio,
        audio_error: state.audio_error.clone(),
    })
}

#[tauri::command]
pub fn library_set_root(state: State<'_, AppState>, path: String) -> Result<Vec<TrackDto>, String> {
    scan_root(state.db.clone(), state.app.clone(), path.into())
}

#[tauri::command]
pub fn library_search(state: State<'_, AppState>, query: String) -> Result<Vec<TrackDto>, String> {
    let conn = db(&state);
    list_tracks(&conn, &query)
}

#[tauri::command]
pub fn play(state: State<'_, AppState>, ids: Vec<i64>, index: usize) -> Result<(), String> {
    let player = player(&state)?;
    let tracks = {
        let conn = db(&state);
        tracks_by_ids(&conn, &ids)?
    };
    let items = tracks.into_iter().map(|track| to_item(track, String::new())).collect();
    player.load_queue(items, index, false);
    Ok(())
}

#[tauri::command]
pub fn play_queue_index(state: State<'_, AppState>, index: usize) -> Result<(), String> {
    player(&state)?.play_index(index);
    Ok(())
}

#[tauri::command]
pub fn toggle(state: State<'_, AppState>) -> Result<(), String> {
    player(&state)?.toggle();
    Ok(())
}

#[tauri::command]
pub fn seek(state: State<'_, AppState>, secs: f64) -> Result<(), String> {
    player(&state)?.seek(secs);
    Ok(())
}

#[tauri::command]
pub fn play_next(state: State<'_, AppState>) -> Result<(), String> {
    player(&state)?.next();
    Ok(())
}

#[tauri::command]
pub fn play_prev(state: State<'_, AppState>) -> Result<(), String> {
    player(&state)?.prev();
    Ok(())
}

#[tauri::command]
pub fn set_volume(state: State<'_, AppState>, volume: f32) -> Result<(), String> {
    let player = player(&state)?;
    player.set_volume(volume);
    let conn = db(&state);
    set_setting(&conn, "volume", &format!("{:.4}", volume.clamp(0.0, 1.0)))?;
    Ok(())
}

#[tauri::command]
pub fn set_repeat(state: State<'_, AppState>, mode: String) -> Result<(), String> {
    player(&state)?.set_repeat(&mode);
    Ok(())
}

#[tauri::command]
pub fn set_shuffle(state: State<'_, AppState>, on: bool) -> Result<(), String> {
    player(&state)?.set_shuffle(on);
    Ok(())
}

#[tauri::command]
pub fn eq_set(state: State<'_, AppState>, bands: Vec<f32>) -> Result<(), String> {
    if bands.len() != 10 {
        return Err("нужно 10 полос".into());
    }
    let mut gains = [0.0_f32; 10];
    for (slot, value) in gains.iter_mut().zip(bands.iter()) {
        *slot = value.clamp(-12.0, 12.0);
    }
    let player = player(&state)?;
    player.set_eq(gains);
    let conn = db(&state);
    set_setting(&conn, "eq", &serde_json::to_string(&gains).unwrap_or_else(|_| "[]".into()))?;
    Ok(())
}

#[tauri::command]
pub fn eq_preset(state: State<'_, AppState>, name: String) -> Result<Vec<f32>, String> {
    let gains = preset(&name).ok_or_else(|| "неизвестный пресет".to_string())?;
    player(&state)?.set_eq(gains);
    let conn = db(&state);
    set_setting(&conn, "eq", &serde_json::to_string(&gains).unwrap_or_else(|_| "[]".into()))?;
    Ok(gains.to_vec())
}

#[tauri::command]
pub fn track_cover(state: State<'_, AppState>, id: i64) -> Result<Option<String>, String> {
    let conn = db(&state);
    Ok(cover_data_url(&conn, id))
}

#[tauri::command]
pub fn radio_start(state: State<'_, AppState>, seed_id: Option<i64>) -> Result<(), String> {
    let player = player(&state)?;
    let seed = seed_id.or_else(|| player.current_track_id()).or_else(|| {
        let conn = db(&state);
        library::first_track_id(&conn)
    });
    let Some(seed) = seed else {
        return Err("В библиотеке пока нет треков".into());
    };
    ensure_analyzed(&state.db, &state.app, Some(seed), 8)?;
    let tracks = {
        let conn = db(&state);
        list_tracks(&conn, "")?
    };
    if tracks.is_empty() {
        return Err("В библиотеке пока нет треков".into());
    }
    let analyzed: Vec<TrackDto> = tracks.iter().filter(|track| track.analyzed).cloned().collect();
    let pool = if analyzed.len() >= 2 { analyzed } else { tracks };
    let seed_index = pool.iter().position(|track| track.id == seed).unwrap_or(0);
    let feats: Vec<Feat> = pool
        .iter()
        .map(|track| Feat {
            bpm: track.bpm,
            energy: track.energy,
            centroid: track.centroid,
        })
        .collect();
    let rng = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    let order = build_radio(seed_index, &feats, 30, rng ^ seed as u64);
    let items = order
        .into_iter()
        .map(|(index, reason)| to_item(pool[index].clone(), reason))
        .collect();
    player.load_queue(items, 0, true);
    Ok(())
}
