# RustyAudioConverter

A cross-platform (macOS + Linux) audio format converter, built with Rust + Tauri v2,
using the system's `ffmpeg`/`ffprobe` to do the actual encoding.

## Architecture

Single-binary Tauri app (no privileged helper needed — unlike RustyWriter, this
doesn't touch raw disks, so it never needs to escalate privileges).

- **`src-tauri/src/ffmpeg.rs`** — detects `ffmpeg`/`ffprobe` on PATH and in common
  install locations (Homebrew on both Apple Silicon/Intel, MacPorts, apt/dnf/pacman/
  zypper/flatpak/snap paths on Linux). GUI apps on macOS in particular often launch
  with a minimal PATH that misses Homebrew, so we check known paths too. Also builds
  platform-specific install instructions by checking which package manager is present.
- **`src-tauri/src/convert.rs`** — probes duration via `ffprobe`, builds the right
  `ffmpeg` codec args per target format, spawns it with `-progress pipe:1` (machine-
  readable key=value progress lines are far easier to parse reliably than scraping
  stderr), and emits a `conversion-progress` Tauri event per line so the frontend can
  drive a progress bar.
- **`src-tauri/src/commands.rs`** — the `#[tauri::command]` surface: check status, get
  install info, verify a manually-located ffmpeg path, probe a file, run a conversion.
- **`src/`** — vanilla HTML/JS/CSS frontend (no framework, to keep the footprint
  small). Native OS drag-and-drop is enabled in `tauri.conf.json` so dropped files
  deliver real filesystem paths via `getCurrentWebview().onDragDropEvent()` — the
  HTML5 `File` API doesn't expose real paths in a webview.

## Supported formats

Any-to-any among: MP3, AAC/M4A, FLAC, WAV, OGG Vorbis, Opus, ALAC, AIFF, WMA, AMR.
Adding a format is a one-line addition to `codec_args_for_format()` in `convert.rs`
plus a matching `<option>` in `index.html`.

## ffmpeg handling

On launch, the app runs `check_ffmpeg`. If it's missing:
- Shows platform-appropriate install commands (only for package managers actually
  detected on the system) with a **copy** button and a **recheck** button — the app
  never silently shells out to a package manager on its own.
- Offers a **manual locate** flow (file picker) for ffmpeg installed somewhere
  nonstandard, verified by actually running `<path> -version`.
- Links to a static-build download page (evermeet.cx for macOS, johnvansickle.com
  for Linux) as a fallback.

## Setup

```bash
npm install
npm run dev      # launches the Tauri dev window
npm run build    # produces platform installers (dmg/app on macOS, deb/rpm/AppImage on Linux)
```

Requires the Tauri v2 system prerequisites (Rust toolchain, and on Linux the
WebKitGTK/GTK dev packages — same ones you already hit while building RustyWriter's
Linux target).

## Known follow-ups / things to firm up

- No icons are included yet (`src-tauri/icons/`) — needed before `tauri build` will
  produce a real bundle; `tauri icon <path-to-1024-png>` generates the full set.
- Bitrate options are currently a fixed dropdown (128/192/256/320); could switch to
  VBR quality presets per codec (e.g. libmp3lame `-q:a`) if you'd rather.
- No batch-level "convert as ZIP" or drag-out-of-app-when-done convenience yet.
- Error surfacing shows raw ffmpeg stderr tail — fine for a dev tool, might want to
  pattern-match common failures (codec not compiled in, permission denied, disk full)
  into friendlier messages later.
