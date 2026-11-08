use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::PictureType;
use lofty::probe::Probe;
use lofty::tag::Accessor;
use rusqlite::Connection;
use tauri::{AppHandle, Emitter};
use walkdir::WalkDir;

use super::db::{
    self, delete_all_tracks, delete_missing, existing_mtime, set_setting, update_features, upsert_track, TagInfo,
    TrackDto,
};
use crate::audio::{analyze_file, Features};

static SCANNING: AtomicBool = AtomicBool::new(false);

const EXTS: &[&str] = &[
    "mp3", "flac", "ogg", "oga", "opus", "wav", "wave", "aiff", "aif", "aac", "m4a", "mp4", "m4b", "wma", "ape",
    "wv", "ac3", "mka", "mpc", "tta", "dsf", "dff", "caf", "spx",
];

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanEvt {
    pub phase: String,
    pub done: u32,
    pub total: u32,
    pub current: String,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeaturesEvt {
    pub id: i64,
    pub bpm: f32,
    pub energy: f32,
    pub centroid: f32,
}

pub fn scan_root(db: Arc<Mutex<Connection>>, app: AppHandle, root: PathBuf) -> Result<Vec<TrackDto>, String> {
    if SCANNING.swap(true, Ordering::SeqCst) {
        return Err("Сканирование уже идёт".into());
    }
    let outcome = (|| {
        if !root.is_dir() {
            return Err("Это не папка".into());
        }
        let root_str = root.to_string_lossy().to_string();
        let files = collect_audio(&root);
        {
            let conn = lock(&db);
            let previous = db::get_setting(&conn, "root");
            if previous.as_deref() != Some(root_str.as_str()) {
                delete_all_tracks(&conn)?;
            }
            set_setting(&conn, "root", &root_str)?;
        }
        let total = files.len() as u32;
        for (index, path) in files.iter().enumerate() {
            let path_str = path.to_string_lossy().to_string();
            let mtime = file_mtime(path);
            let unchanged = {
                let conn = lock(&db);
                existing_mtime(&conn, &path_str) == Some(mtime)
            };
            if !unchanged {
                let info = read_tags(path);
                let conn = lock(&db);
                upsert_track(&conn, &path_str, mtime, &info)?;
            }
            if index % 8 == 0 || index + 1 == files.len() {
                let _ = app.emit(
                    "scan",
                    ScanEvt {
                        phase: "tags".into(),
                        done: (index + 1) as u32,
                        total,
                        current: file_name(path),
                    },
                );
            }
        }
        let seen: Vec<String> = files.iter().map(|path| path.to_string_lossy().to_string()).collect();
        {
            let conn = lock(&db);
            delete_missing(&conn, &root_str, &seen)?;
        }
        let tracks = {
            let conn = lock(&db);
            db::list_tracks(&conn, "")?
        };
        let db_bg = db.clone();
        let app_bg = app.clone();
        std::thread::spawn(move || {
            let _ = analyze_pending(&db_bg, &app_bg, 0);
            SCANNING.store(false, Ordering::SeqCst);
            let _ = app_bg.emit(
                "scan",
                ScanEvt {
                    phase: "done".into(),
                    done: 0,
                    total: 0,
                    current: String::new(),
                },
            );
        });
        Ok(tracks)
    })();
    if outcome.is_err() {
        SCANNING.store(false, Ordering::SeqCst);
    }
    outcome
}

pub fn ensure_analyzed(db: &Mutex<Connection>, app: &AppHandle, seed: Option<i64>, min_ready: usize) -> Result<(), String> {
    if let Some(id) = seed {
        let path = {
            let conn = lock(db);
            db::track_path(&conn, id)
        };
        if let Some((path, analyzed)) = path {
            if !analyzed {
                store_analysis(db, app, id, &path);
            }
        }
    }
    let ready = {
        let conn = lock(db);
        db::list_tracks(&conn, "")?.into_iter().filter(|track| track.analyzed).count()
    };
    if ready >= min_ready {
        return Ok(());
    }
    let need = 40;
    let batch = {
        let conn = lock(db);
        db::unanalyzed(&conn, need)?
    };
    for (index, (id, path)) in batch.iter().enumerate() {
        let _ = app.emit(
            "scan",
            ScanEvt {
                phase: "analyze".into(),
                done: index as u32,
                total: batch.len() as u32,
                current: file_name(Path::new(path)),
            },
        );
        store_analysis(db, app, *id, path);
    }
    Ok(())
}

fn analyze_pending(db: &Mutex<Connection>, app: &AppHandle, _extra: usize) -> Result<(), String> {
    loop {
        let batch = {
            let conn = lock(db);
            db::unanalyzed(&conn, 12)?
        };
        if batch.is_empty() {
            break;
        }
        let total = batch.len() as u32;
        for (index, (id, path)) in batch.iter().enumerate() {
            let _ = app.emit(
                "scan",
                ScanEvt {
                    phase: "analyze".into(),
                    done: index as u32,
                    total,
                    current: file_name(Path::new(path)),
                },
            );
            store_analysis(db, app, *id, path);
        }
    }
    Ok(())
}

fn store_analysis(db: &Mutex<Connection>, app: &AppHandle, id: i64, path: &str) {
    let features = analyze_file(Path::new(path)).unwrap_or(Features {
        bpm: 0.0,
        energy: 0.0,
        centroid: 0.0,
    });
    let conn = lock(db);
    let _ = update_features(&conn, id, features.bpm, features.energy, features.centroid);
    drop(conn);
    let _ = app.emit(
        "features",
        FeaturesEvt {
            id,
            bpm: features.bpm,
            energy: features.energy,
            centroid: features.centroid,
        },
    );
}

fn collect_audio(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in WalkDir::new(root).follow_links(false).into_iter().filter_map(|entry| entry.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.components().any(|component| {
            component
                .as_os_str()
                .to_string_lossy()
                .starts_with('.')
        }) {
            continue;
        }
        let ext = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if EXTS.contains(&ext.as_str()) {
            files.push(path.to_path_buf());
        }
    }
    files.sort();
    files
}

fn read_tags(path: &Path) -> TagInfo {
    let fallback = file_name(path);
    match Probe::open(path).and_then(|mut probe| probe.read()) {
        Ok(file) => {
            let duration = file.properties().duration().as_secs_f64();
            let tag = file.primary_tag();
            let title = tag.and_then(|tag| tag.title()).map(|v| v.to_string()).filter(|v| !v.is_empty());
            let artist = tag.and_then(|tag| tag.artist()).map(|v| v.to_string()).filter(|v| !v.is_empty());
            let album = tag.and_then(|tag| tag.album()).map(|v| v.to_string()).filter(|v| !v.is_empty());
            let cover = tag.and_then(|tag| {
                tag.pictures()
                    .iter()
                    .find(|picture| picture.pic_type() == PictureType::CoverFront)
                    .or_else(|| tag.pictures().first())
                    .map(|picture| picture.data().to_vec())
                    .filter(|bytes| bytes.len() > 32 && bytes.len() < 4_000_000)
            });
            let cover_mime = cover.as_ref().map(|bytes| sniff_mime(bytes).to_string());
            TagInfo {
                title: title.unwrap_or(fallback),
                artist: artist.unwrap_or_else(|| "Неизвестный исполнитель".into()),
                album: album.unwrap_or_default(),
                duration,
                cover,
                cover_mime,
            }
        }
        Err(_) => TagInfo {
            title: fallback,
            artist: "Неизвестный исполнитель".into(),
            album: String::new(),
            duration: 0.0,
            cover: None,
            cover_mime: None,
        },
    }
}

fn sniff_mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else if bytes.starts_with(b"\x89PNG") {
        "image/png"
    } else {
        "application/octet-stream"
    }
}

fn file_mtime(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn file_name(path: &Path) -> String {
    path.file_stem()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "Без названия".into())
}

fn lock(db: &Mutex<Connection>) -> std::sync::MutexGuard<'_, Connection> {
    db.lock().unwrap_or_else(|err| err.into_inner())
}
