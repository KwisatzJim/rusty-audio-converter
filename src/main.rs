// Prevents an additional console window on Windows in release builds. No-op elsewhere.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod convert;
mod ffmpeg;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::check_ffmpeg,
            commands::get_install_info,
            commands::verify_custom_ffmpeg_path,
            commands::probe_file,
            commands::convert_audio,
        ])
        .run(tauri::generate_context!())
        .expect("error while running RustyAudioConverter");
}
