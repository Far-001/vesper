use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use tauri::AppHandle;

use crate::audio::Player;

pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub player: Option<Player>,
    pub audio_error: Option<String>,
    pub app: AppHandle,
}
