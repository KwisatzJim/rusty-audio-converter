use crate::convert::{self, ConvertOptions};
use crate::ffmpeg;
use serde::Serialize;
use tauri::AppHandle;

#[tauri::command]
pub fn check_ffmpeg() -> ffmpeg::FfmpegStatus {
    ffmpeg::check_status()
}

#[tauri::command]
pub fn get_install_info() -> ffmpeg::InstallInfo {
    ffmpeg::get_install_info()
}

/// Lets the user point at a custom ffmpeg location if it's installed somewhere
/// nonstandard (e.g. a self-built binary, or a path outside our known list).
#[tauri::command]
pub fn verify_custom_ffmpeg_path(path: String) -> Result<ffmpeg::FfmpegStatus, String> {
    let p = std::path::PathBuf::from(&path);
    if !p.is_file() {
        return Err("That path doesn't point to a file.".into());
    }
    let output = std::process::Command::new(&p)
        .arg("-version")
        .output()
        .map_err(|e| format!("Couldn't execute that file: {e}"))?;
    if !output.status.success() {
        return Err("That file didn't run successfully as ffmpeg.".into());
    }

    // Look for ffprobe alongside it.
    let ffprobe_candidate = p.with_file_name(if cfg!(target_os = "windows") {
        "ffprobe.exe"
    } else {
        "ffprobe"
    });
    let ffprobe_found = ffprobe_candidate.is_file();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = stdout.lines().next().map(|s| s.to_string());

    Ok(ffmpeg::FfmpegStatus {
        ffmpeg_found: true,
        ffmpeg_path: Some(p.to_string_lossy().to_string()),
        ffprobe_found,
        ffprobe_path: if ffprobe_found {
            Some(ffprobe_candidate.to_string_lossy().to_string())
        } else {
            None
        },
        version,
    })
}

#[derive(Serialize)]
pub struct ProbeResult {
    pub duration_secs: Option<f64>,
}

#[tauri::command]
pub async fn probe_file(ffprobe_path: String, input_path: String) -> ProbeResult {
    let duration_secs = convert::probe_duration_secs(&ffprobe_path, &input_path).await;
    ProbeResult { duration_secs }
}

#[tauri::command]
pub async fn convert_audio(
    app: AppHandle,
    job_id: String,
    ffmpeg_path: String,
    ffprobe_path: String,
    options: ConvertOptions,
) -> Result<(), String> {
    convert::run_conversion(app, job_id, ffmpeg_path, ffprobe_path, options).await
}
