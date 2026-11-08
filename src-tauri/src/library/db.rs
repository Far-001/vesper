use std::path::Path;

use rusqlite::{params, Connection};
use serde::Serialize;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackDto {
    pub id: i64,
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
    pub bpm: f32,
    pub energy: f32,
    pub centroid: f32,
    pub analyzed: bool,
}

pub struct TagInfo {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
    pub cover: Option<Vec<u8>>,
    pub cover_mime: Option<String>,
}

pub fn open_db(path: &Path) -> Result<Connection, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let conn = Connection::open(path).map_err(|err| err.to_string())?;
    conn.execute_batch(
        "
        PRAGMA journal_mode=WAL;
        PRAGMA synchronous=NORMAL;
        CREATE TABLE IF NOT EXISTS tracks (
            id INTEGER PRIMARY KEY,
            path TEXT NOT NULL UNIQUE,
            title TEXT NOT NULL,
            artist TEXT NOT NULL,
            album TEXT NOT NULL,
            duration REAL NOT NULL DEFAULT 0,
            mtime INTEGER NOT NULL DEFAULT 0,
            cover BLOB,
            cover_mime TEXT,
            bpm REAL NOT NULL DEFAULT 0,
            energy REAL NOT NULL DEFAULT 0,
            centroid REAL NOT NULL DEFAULT 0,
            analyzed INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        ",
    )
    .map_err(|err| err.to_string())?;
    Ok(conn)
}

pub fn get_setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| row.get(0))
        .ok()
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .map(|_| ())
    .map_err(|err| err.to_string())
}

pub fn list_tracks(conn: &Connection, query: &str) -> Result<Vec<TrackDto>, String> {
    let trimmed = query.trim();
    let like = format!("%{}%", trimmed.replace('%', "").replace('_', ""));
    let mut stmt = conn
        .prepare(
            "SELECT id, path, title, artist, album, duration, bpm, energy, centroid, analyzed
             FROM tracks
             WHERE ?1 = '' OR title LIKE ?2 OR artist LIKE ?2 OR album LIKE ?2
             ORDER BY artist COLLATE NOCASE, album COLLATE NOCASE, title COLLATE NOCASE
             LIMIT 800",
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map(params![trimmed, like], row_to_track)
        .map_err(|err| err.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|err| err.to_string())
}

pub fn tracks_by_ids(conn: &Connection, ids: &[i64]) -> Result<Vec<TrackDto>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut found = std::collections::HashMap::new();
    for chunk in ids.chunks(200) {
        let marks = vec!["?"; chunk.len()].join(",");
        let sql = format!(
            "SELECT id, path, title, artist, album, duration, bpm, energy, centroid, analyzed
             FROM tracks WHERE id IN ({marks})"
        );
        let mut stmt = conn.prepare(&sql).map_err(|err| err.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(chunk.iter()), row_to_track)
            .map_err(|err| err.to_string())?;
        for row in rows {
            let track = row.map_err(|err| err.to_string())?;
            found.insert(track.id, track);
        }
    }
    Ok(ids.iter().filter_map(|id| found.get(id).cloned()).collect())
}

pub fn first_track_id(conn: &Connection) -> Option<i64> {
    conn.query_row("SELECT id FROM tracks ORDER BY id LIMIT 1", [], |row| row.get(0))
        .ok()
}

pub fn unanalyzed(conn: &Connection, limit: usize) -> Result<Vec<(i64, String)>, String> {
    let mut stmt = conn
        .prepare("SELECT id, path FROM tracks WHERE analyzed = 0 ORDER BY id LIMIT ?1")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([limit as i64], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|err| err.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|err| err.to_string())
}

pub fn track_path(conn: &Connection, id: i64) -> Option<(String, bool)> {
    conn.query_row(
        "SELECT path, analyzed FROM tracks WHERE id = ?1",
        [id],
        |row| Ok((row.get(0)?, row.get::<_, i64>(1)? != 0)),
    )
    .ok()
}

pub fn existing_mtime(conn: &Connection, path: &str) -> Option<i64> {
    conn.query_row("SELECT mtime FROM tracks WHERE path = ?1", [path], |row| row.get(0))
        .ok()
}

pub fn upsert_track(conn: &Connection, path: &str, mtime: i64, info: &TagInfo) -> Result<(), String> {
    conn.execute(
        "INSERT INTO tracks (path, title, artist, album, duration, mtime, cover, cover_mime, bpm, energy, centroid, analyzed)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, 0, 0, 0)
         ON CONFLICT(path) DO UPDATE SET
            title = excluded.title,
            artist = excluded.artist,
            album = excluded.album,
            duration = excluded.duration,
            mtime = excluded.mtime,
            cover = COALESCE(excluded.cover, tracks.cover),
            cover_mime = COALESCE(excluded.cover_mime, tracks.cover_mime),
            analyzed = CASE WHEN tracks.mtime = excluded.mtime THEN tracks.analyzed ELSE 0 END,
            bpm = CASE WHEN tracks.mtime = excluded.mtime THEN tracks.bpm ELSE 0 END,
            energy = CASE WHEN tracks.mtime = excluded.mtime THEN tracks.energy ELSE 0 END,
            centroid = CASE WHEN tracks.mtime = excluded.mtime THEN tracks.centroid ELSE 0 END",
        params![
            path,
            info.title,
            info.artist,
            info.album,
            info.duration,
            mtime,
            info.cover,
            info.cover_mime,
        ],
    )
    .map(|_| ())
    .map_err(|err| err.to_string())
}

pub fn update_features(conn: &Connection, id: i64, bpm: f32, energy: f32, centroid: f32) -> Result<(), String> {
    conn.execute(
        "UPDATE tracks SET bpm = ?1, energy = ?2, centroid = ?3, analyzed = 1 WHERE id = ?4",
        params![bpm, energy, centroid, id],
    )
    .map(|_| ())
    .map_err(|err| err.to_string())
}

pub fn delete_all_tracks(conn: &Connection) -> Result<(), String> {
    conn.execute("DELETE FROM tracks", [])
        .map(|_| ())
        .map_err(|err| err.to_string())
}

pub fn delete_missing(conn: &Connection, root: &str, seen: &[String]) -> Result<(), String> {
    let prefix = if root.ends_with('/') {
        root.to_string()
    } else {
        format!("{root}/")
    };
    let mut stmt = conn
        .prepare("SELECT id, path FROM tracks")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))
        .map_err(|err| err.to_string())?;
    let mut stale = Vec::new();
    for row in rows {
        let (id, path) = row.map_err(|err| err.to_string())?;
        if (path == root || path.starts_with(&prefix)) && !seen.iter().any(|item| item == &path) {
            stale.push(id);
        }
    }
    for id in stale {
        conn.execute("DELETE FROM tracks WHERE id = ?1", [id])
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}

pub fn cover_data_url(conn: &Connection, id: i64) -> Option<String> {
    let (mime, blob): (Option<String>, Option<Vec<u8>>) = conn
        .query_row(
            "SELECT cover_mime, cover FROM tracks WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok()?;
    let blob = blob.filter(|bytes| !bytes.is_empty())?;
    let mime = mime.unwrap_or_else(|| sniff(&blob).to_string());
    Some(format!("data:{mime};base64,{}", b64(&blob)))
}

fn row_to_track(row: &rusqlite::Row<'_>) -> rusqlite::Result<TrackDto> {
    Ok(TrackDto {
        id: row.get(0)?,
        path: row.get(1)?,
        title: row.get(2)?,
        artist: row.get(3)?,
        album: row.get(4)?,
        duration: row.get(5)?,
        bpm: row.get(6)?,
        energy: row.get(7)?,
        centroid: row.get(8)?,
        analyzed: row.get::<_, i64>(9)? != 0,
    })
}

fn sniff(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else if bytes.starts_with(b"\x89PNG") {
        "image/png"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

fn b64(data: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8) | data[i + 2] as u32;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
        out.push(TABLE[(n & 63) as usize] as char);
        i += 3;
    }
    if i < data.len() {
        let remain = data.len() - i;
        let mut n = (data[i] as u32) << 16;
        if remain == 2 {
            n |= (data[i + 1] as u32) << 8;
        }
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        if remain == 2 {
            out.push(TABLE[((n >> 6) & 63) as usize] as char);
            out.push('=');
        } else {
            out.push('=');
            out.push('=');
        }
    }
    out
}
