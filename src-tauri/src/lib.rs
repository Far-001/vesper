mod audio;
mod commands;
mod library;
mod radio;
mod state;

use tauri::Manager;

use crate::commands::{
    bootstrap, eq_preset, eq_set, library_search, library_set_root, play, play_next, play_prev,
    play_queue_index, radio_start, saved_eq, saved_volume, seek, set_repeat, set_shuffle, set_volume,
    toggle, track_cover,
};
use crate::state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let conn = library::open_db(&dir.join("vesper.db"))?;
            let volume = saved_volume(&conn);
            let eq = saved_eq(&conn);
            let handle = app.handle().clone();
            let (player, audio_error) = match audio::Player::start(handle.clone(), volume, eq) {
                Ok(player) => (Some(player), None),
                Err(err) => {
                    eprintln!("audio: {err}");
                    (None, Some(err))
                }
            };
            app.manage(AppState {
                db: std::sync::Arc::new(std::sync::Mutex::new(conn)),
                player,
                audio_error,
                app: handle,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            library_set_root,
            library_search,
            play,
            play_queue_index,
            toggle,
            seek,
            play_next,
            play_prev,
            set_volume,
            set_repeat,
            set_shuffle,
            eq_set,
            eq_preset,
            track_cover,
            radio_start,
        ])
        .run(tauri::generate_context!())
        .expect("не удалось запустить Vesper");
}
