use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Stdio;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

#[derive(Deserialize, Clone, Debug)]
pub struct ConvertOptions {
    pub input_path: String,
    pub output_path: String,
    pub format: String, // target extension, e.g. "mp3", "flac", "opus"
    pub bitrate_kbps: Option<u32>, // for lossy formats
    pub sample_rate: Option<u32>,
    pub channels: Option<u32>,
}

#[derive(Serialize, Clone, Debug)]
pub struct ConvertProgress {
    pub job_id: String,
    pub percent: f64,
    pub out_time_secs: f64,
    pub speed: Option<String>,
    pub done: bool,
    pub error: Option<String>,
}

/// Maps a target file extension to the ffmpeg args needed to encode it.
/// Returns (codec_args, requires_bitrate).
fn codec_args_for_format(format: &str) -> Vec<String> {
    let f = format.to_lowercase();
    match f.as_str() {
        "mp3" => vec!["-c:a".into(), "libmp3lame".into()],
        "aac" | "m4a" => vec!["-c:a".into(), "aac".into(), "-movflags".into(), "+faststart".into()],
        "ogg" => vec!["-c:a".into(), "libvorbis".into()],
        "opus" => vec!["-c:a".into(), "libopus".into()],
        "flac" => vec!["-c:a".into(), "flac".into()],
        "wav" => vec!["-c:a".into(), "pcm_s16le".into()],
        "aiff" | "aif" => vec!["-c:a".into(), "pcm_s16be".into()],
        "alac" => vec!["-c:a".into(), "alac".into()],
        "wma" => vec!["-c:a".into(), "wmav2".into()],
        "amr" => vec!["-c:a".into(), "libopencore_amrnb".into(), "-ar".into(), "8000".into(), "-ac".into(), "1".into()],
        // Fallback: let ffmpeg pick a default encoder for the container.
        _ => vec![],
    }
}

fn is_lossy(format: &str) -> bool {
    matches!(
        format.to_lowercase().as_str(),
        "mp3" | "aac" | "m4a" | "ogg" | "opus" | "wma" | "amr"
    )
}

/// Runs `ffprobe` to get duration in seconds, used to compute conversion percentage.
pub async fn probe_duration_secs(ffprobe_path: &str, input_path: &str) -> Option<f64> {
    let output = Command::new(ffprobe_path)
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            input_path,
        ])
        .output()
        .await
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.trim().parse::<f64>().ok()
}

/// Parses a single `-progress pipe:1` key=value line, updating the running state.
/// ffmpeg emits blocks of lines ending in `progress=continue` or `progress=end`.
struct ProgressState {
    out_time_ms: f64,
    speed: Option<String>,
}

pub async fn run_conversion(
    app: AppHandle,
    job_id: String,
    ffmpeg_path: String,
    ffprobe_path: String,
    options: ConvertOptions,
) -> Result<(), String> {
    let duration = probe_duration_secs(&ffprobe_path, &options.input_path).await;

    if let Some(parent) = Path::new(&options.output_path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }

    let mut args: Vec<String> = vec![
        "-y".into(),
        "-i".into(),
        options.input_path.clone(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
    ];

    args.extend(codec_args_for_format(&options.format));

    if is_lossy(&options.format) {
        if let Some(kbps) = options.bitrate_kbps {
            args.push("-b:a".into());
            args.push(format!("{kbps}k"));
        }
    }
    if let Some(rate) = options.sample_rate {
        args.push("-ar".into());
        args.push(rate.to_string());
    }
    if let Some(ch) = options.channels {
        args.push("-ac".into());
        args.push(ch.to_string());
    }

    args.push("-progress".into());
    args.push("pipe:1".into());
    args.push(options.output_path.clone());

    let mut child = Command::new(&ffmpeg_path)
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to launch ffmpeg: {e}"))?;

    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr = child.stderr.take().ok_or("no stderr")?;

    let mut stdout_reader = BufReader::new(stdout).lines();
    let mut stderr_reader = BufReader::new(stderr).lines();

    let mut state = ProgressState {
        out_time_ms: 0.0,
        speed: None,
    };

    let mut stderr_output = String::new();

    loop {
        tokio::select! {
            line = stdout_reader.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        if let Some((key, value)) = line.split_once('=') {
                            match key {
                                "out_time_ms" => {
                                    if let Ok(v) = value.trim().parse::<f64>() {
                                        state.out_time_ms = v;
                                    }
                                }
                                "out_time_us" => {
                                    if let Ok(v) = value.trim().parse::<f64>() {
                                        state.out_time_ms = v; // microseconds; treated consistently below
                                    }
                                }
                                "speed" => {
                                    state.speed = Some(value.trim().to_string());
                                }
                                "progress" => {
                                    let out_time_secs = state.out_time_ms / 1_000_000.0;
                                    let percent = match duration {
                                        Some(d) if d > 0.0 => (out_time_secs / d * 100.0).clamp(0.0, 100.0),
                                        _ => 0.0,
                                    };
                                    let done = value.trim() == "end";
                                    let _ = app.emit(
                                        "conversion-progress",
                                        ConvertProgress {
                                            job_id: job_id.clone(),
                                            percent: if done { 100.0 } else { percent },
                                            out_time_secs,
                                            speed: state.speed.clone(),
                                            done,
                                            error: None,
                                        },
                                    );
                                }
                                _ => {}
                            }
                        }
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
            line = stderr_reader.next_line() => {
                if let Ok(Some(line)) = line {
                    stderr_output.push_str(&line);
                    stderr_output.push('\n');
                }
            }
        }
    }

    let status = child.wait().await.map_err(|e| e.to_string())?;

    if !status.success() {
        let err_msg = if stderr_output.trim().is_empty() {
            format!("ffmpeg exited with status {status}")
        } else {
            stderr_output.trim().to_string()
        };
        let _ = app.emit(
            "conversion-progress",
            ConvertProgress {
                job_id: job_id.clone(),
                percent: 0.0,
                out_time_secs: 0.0,
                speed: None,
                done: true,
                error: Some(err_msg.clone()),
            },
        );
        return Err(err_msg);
    }

    Ok(())
}
