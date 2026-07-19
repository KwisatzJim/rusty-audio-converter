use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;

/// Extra locations to check beyond PATH, since GUI apps on macOS in particular
/// often launch with a stripped-down PATH that doesn't include Homebrew.
fn candidate_paths() -> Vec<PathBuf> {
    let mut paths = vec![];

    #[cfg(target_os = "macos")]
    {
        paths.push(PathBuf::from("/opt/homebrew/bin/ffmpeg")); // Apple Silicon brew
        paths.push(PathBuf::from("/usr/local/bin/ffmpeg")); // Intel brew
        paths.push(PathBuf::from("/opt/local/bin/ffmpeg")); // MacPorts
    }

    #[cfg(target_os = "linux")]
    {
        paths.push(PathBuf::from("/usr/bin/ffmpeg"));
        paths.push(PathBuf::from("/usr/local/bin/ffmpeg"));
        paths.push(PathBuf::from("/snap/bin/ffmpeg"));
        paths.push(PathBuf::from("/var/lib/flatpak/exports/bin/ffmpeg"));
        if let Ok(home) = std::env::var("HOME") {
            paths.push(PathBuf::from(format!(
                "{home}/.local/share/flatpak/exports/bin/ffmpeg"
            )));
        }
    }

    paths
}

fn binary_name(base: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        format!("{base}.exe")
    }
    #[cfg(not(target_os = "windows"))]
    {
        base.to_string()
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct FfmpegStatus {
    pub ffmpeg_found: bool,
    pub ffmpeg_path: Option<String>,
    pub ffprobe_found: bool,
    pub ffprobe_path: Option<String>,
    pub version: Option<String>,
}

/// Locate a binary by name: checks PATH first (via `which`), then a list of
/// common install locations that may not be on PATH for a GUI-launched app.
pub fn find_binary(name: &str) -> Option<PathBuf> {
    let exe_name = binary_name(name);

    if let Ok(found) = which::which(&exe_name) {
        return Some(found);
    }

    for candidate in candidate_paths() {
        let candidate = if candidate.file_name().and_then(|f| f.to_str()) == Some("ffmpeg")
            && name != "ffmpeg"
        {
            candidate.with_file_name(binary_name(name))
        } else {
            candidate
        };
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

fn get_version(ffmpeg_path: &PathBuf) -> Option<String> {
    let output = Command::new(ffmpeg_path).arg("-version").output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    // First line looks like: "ffmpeg version 6.1.1 Copyright (c) 2000-2023 ..."
    let first_line = stdout.lines().next()?;
    Some(first_line.to_string())
}

pub fn check_status() -> FfmpegStatus {
    let ffmpeg_path = find_binary("ffmpeg");
    let ffprobe_path = find_binary("ffprobe");

    let version = ffmpeg_path.as_ref().and_then(get_version);

    FfmpegStatus {
        ffmpeg_found: ffmpeg_path.is_some(),
        ffmpeg_path: ffmpeg_path.map(|p| p.to_string_lossy().to_string()),
        ffprobe_found: ffprobe_path.is_some(),
        ffprobe_path: ffprobe_path.map(|p| p.to_string_lossy().to_string()),
        version,
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct InstallOption {
    pub label: String,
    pub command: String,
    pub note: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct InstallInfo {
    pub platform: String,
    pub options: Vec<InstallOption>,
    pub manual_download_url: String,
}

/// Returns platform-appropriate ways to install ffmpeg, in order of preference.
/// We never auto-run a package manager install without the user explicitly
/// clicking a specific option in the UI (see commands::run_install_command).
pub fn get_install_info() -> InstallInfo {
    #[cfg(target_os = "macos")]
    {
        let mut options = vec![];
        if which::which("brew").is_ok() {
            options.push(InstallOption {
                label: "Homebrew".into(),
                command: "brew install ffmpeg".into(),
                note: "Recommended. Installs ffmpeg and ffprobe.".into(),
            });
        } else {
            options.push(InstallOption {
                label: "Homebrew (not detected)".into(),
                command: "/bin/bash -c \"$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\" && brew install ffmpeg".into(),
                note: "Installs Homebrew first, then ffmpeg. Takes a few minutes.".into(),
            });
        }
        if which::which("port").is_ok() {
            options.push(InstallOption {
                label: "MacPorts".into(),
                command: "sudo port install ffmpeg".into(),
                note: "Requires admin password.".into(),
            });
        }
        InstallInfo {
            platform: "macos".into(),
            options,
            manual_download_url: "https://evermeet.cx/ffmpeg/".into(),
        }
    }

    #[cfg(target_os = "linux")]
    {
        let mut options = vec![];
        let managers: &[(&str, &str, &str)] = &[
            ("apt", "sudo apt update && sudo apt install -y ffmpeg", "Debian, Ubuntu, and derivatives"),
            ("dnf", "sudo dnf install -y ffmpeg", "Fedora (needs RPM Fusion enabled for full codec support)"),
            ("pacman", "sudo pacman -S --noconfirm ffmpeg", "Arch, Manjaro"),
            ("zypper", "sudo zypper install -y ffmpeg", "openSUSE"),
        ];
        for (bin, cmd, note) in managers {
            if which::which(bin).is_ok() {
                options.push(InstallOption {
                    label: (*bin).into(),
                    command: (*cmd).into(),
                    note: (*note).into(),
                });
            }
        }
        if which::which("flatpak").is_ok() {
            options.push(InstallOption {
                label: "Flatpak".into(),
                command: "flatpak install -y flathub org.freedesktop.Platform.ffmpeg-full".into(),
                note: "Sandboxed runtime extension; useful if your distro's repo lacks codecs."
                    .into(),
            });
        }
        if options.is_empty() {
            options.push(InstallOption {
                label: "apt (assumed)".into(),
                command: "sudo apt update && sudo apt install -y ffmpeg".into(),
                note: "No known package manager was detected on PATH; this is a common default."
                    .into(),
            });
        }
        InstallInfo {
            platform: "linux".into(),
            options,
            manual_download_url: "https://johnvansickle.com/ffmpeg/".into(),
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        InstallInfo {
            platform: "other".into(),
            options: vec![],
            manual_download_url: "https://ffmpeg.org/download.html".into(),
        }
    }
}
