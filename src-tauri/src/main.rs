// Meowverter - a cute, simple OLED-dark GUI for FFmpeg.
// Keep the console in debug builds (handy for logs); hide it in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// BtbN's rolling "latest" GPL build - includes x264, x265, vpx/vp9, opus, mp3lame, etc.
const FFMPEG_URL: &str =
    "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip";
const YTDLP_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
const DENO_URL: &str =
    "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip";

#[derive(Default)]
struct AppState {
    ffmpeg: Mutex<Option<PathBuf>>,
    ffprobe: Mutex<Option<PathBuf>>,
    child: Mutex<Option<Child>>,
    cancel: Mutex<bool>,
    active_job: Mutex<bool>,
    tool_setup: Mutex<()>,
    hw_cache: Mutex<HashMap<String, bool>>, // per-encoder "does this machine support it?"
}

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn new_cmd<P: AsRef<std::ffi::OsStr>>(program: P) -> Command {
    let mut c = Command::new(program);
    #[cfg(windows)]
    c.creation_flags(CREATE_NO_WINDOW);
    c
}

fn bin_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("Meowverter")
        .join("bin")
}

fn temp_path(prefix: &str, extension: &str) -> PathBuf {
    let n = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let suffix = if extension.is_empty() {
        String::new()
    } else {
        format!(".{extension}")
    };
    std::env::temp_dir().join(format!("{prefix}_{}_{}{suffix}", std::process::id(), n))
}

/// Returns Some(path) if `exe -version` runs successfully (i.e. it's on PATH).
fn on_path(exe: &str) -> Option<PathBuf> {
    let ok = new_cmd(exe)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    ok.then(|| PathBuf::from(exe))
}

/// Prefer Meowverter's bundled tools. This avoids launching several `-version`
/// checks on every startup and ensures the updater manages the tools we use.
fn locate_tools() -> (Option<PathBuf>, Option<PathBuf>) {
    let local_mpeg = bin_dir().join("ffmpeg.exe");
    let local_probe = bin_dir().join("ffprobe.exe");
    let ffmpeg = local_mpeg
        .exists()
        .then_some(local_mpeg)
        .or_else(|| on_path("ffmpeg"));
    let ffprobe = local_probe
        .exists()
        .then_some(local_probe)
        .or_else(|| on_path("ffprobe"));
    (ffmpeg, ffprobe)
}

fn claim_job(state: &AppState) -> Result<(), String> {
    let mut active = state.active_job.lock().unwrap();
    if *active {
        return Err("Another conversion or download is already running.".into());
    }
    *active = true;
    Ok(())
}

struct ActiveJob<'a>(&'a AppState);

impl Drop for ActiveJob<'_> {
    fn drop(&mut self) {
        *self.0.active_job.lock().unwrap() = false;
    }
}

fn stored_ffmpeg(state: &AppState) -> Result<PathBuf, String> {
    if let Some(p) = state.ffmpeg.lock().unwrap().clone() {
        return Ok(p);
    }
    let (m, p) = locate_tools();
    *state.ffmpeg.lock().unwrap() = m.clone();
    *state.ffprobe.lock().unwrap() = p;
    m.ok_or_else(|| "ffmpeg not found - let Meowverter download it first.".into())
}

fn stored_ffprobe(state: &AppState) -> Result<PathBuf, String> {
    if let Some(p) = state.ffprobe.lock().unwrap().clone() {
        return Ok(p);
    }
    let (m, p) = locate_tools();
    *state.ffmpeg.lock().unwrap() = m;
    *state.ffprobe.lock().unwrap() = p.clone();
    p.ok_or_else(|| "ffprobe not found - let Meowverter download it first.".into())
}

/// Whether NVIDIA NVENC (hevc) encoding works here. Tested once, then cached.
/// Does this machine's ffmpeg actually encode with the given hardware encoder?
/// Runs a tiny throwaway encode and caches the yes/no per encoder name. This is
/// how we adapt to whatever GPU the user has (NVIDIA/AMD/Intel) without needing
/// to know the model.
fn hw_ok(state: &AppState, ff: &Path, encoder: &str) -> bool {
    if let Some(v) = state.hw_cache.lock().unwrap().get(encoder) {
        return *v;
    }
    let ok = new_cmd(ff)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=256x256:d=0.1",
            "-c:v",
            encoder,
            "-f",
            "null",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    state
        .hw_cache
        .lock()
        .unwrap()
        .insert(encoder.to_string(), ok);
    ok
}

#[derive(Clone, Copy, PartialEq)]
enum Vendor {
    Nvenc, // NVIDIA
    Amf,   // AMD
    Qsv,   // Intel
    Cpu,
}

#[derive(Clone, Copy, PartialEq)]
enum Family {
    Hevc,
    Av1,
    Vp9,
    H264,
}

#[derive(Clone, Copy)]
struct VideoEnc {
    vendor: Vendor,
    name: &'static str, // ffmpeg encoder
    family: Family,
}

/// The CPU encoder for a format (the always-available fallback).
fn cpu_encoder(format: &str) -> VideoEnc {
    match format {
        "mp4_h265" => VideoEnc {
            vendor: Vendor::Cpu,
            name: "libx265",
            family: Family::Hevc,
        },
        "mp4_av1" => VideoEnc {
            vendor: Vendor::Cpu,
            name: "libsvtav1",
            family: Family::Av1,
        },
        "webm" => VideoEnc {
            vendor: Vendor::Cpu,
            name: "libvpx-vp9",
            family: Family::Vp9,
        },
        _ => VideoEnc {
            vendor: Vendor::Cpu,
            name: "libx264",
            family: Family::H264,
        },
    }
}

/// Pick the fastest working encoder for the format: probe NVIDIA, then AMD, then
/// Intel hardware; fall back to CPU. Result is cached per encoder in hw_ok.
fn pick_encoder(state: &AppState, ff: &Path, format: &str) -> VideoEnc {
    let (fam, cands): (Family, &[(Vendor, &'static str)]) = match format {
        "mp4_h265" => (
            Family::Hevc,
            &[
                (Vendor::Nvenc, "hevc_nvenc"),
                (Vendor::Amf, "hevc_amf"),
                (Vendor::Qsv, "hevc_qsv"),
            ],
        ),
        // av1_amf is left out on purpose: its QP scale differs (0-255) and we
        // can't verify it, so AMD AV1 falls back to CPU rather than risk bad output
        "mp4_av1" => (
            Family::Av1,
            &[(Vendor::Nvenc, "av1_nvenc"), (Vendor::Qsv, "av1_qsv")],
        ),
        _ => return cpu_encoder(format),
    };
    for (v, name) in cands {
        if hw_ok(state, ff, name) {
            return VideoEnc {
                vendor: *v,
                name,
                family: fam,
            };
        }
    }
    cpu_encoder(format)
}

/// Constant-quality number for a hardware encoder (roughly a CRF; lower = better).
fn quality_number(fam: Family, quality: &str) -> String {
    let n = match (fam, quality) {
        (Family::Av1, "high") => 27,
        (Family::Av1, "small") => 42,
        (Family::Av1, _) => 33,
        (_, "high") => 23,
        (_, "small") => 33,
        (_, _) => 28,
    };
    n.to_string()
}

/// Hardware constant-quality args (`-c:v <enc> ...`), vendor-specific.
fn hw_quality_args(venc: &VideoEnc, quality: &str) -> Vec<String> {
    let q = quality_number(venc.family, quality);
    let name = venc.name.to_string();
    let v = |s: &str| s.to_string();
    match venc.vendor {
        Vendor::Nvenc => vec![
            v("-c:v"),
            name,
            v("-preset"),
            v("p6"),
            v("-rc"),
            v("vbr"),
            v("-cq"),
            q,
            v("-b:v"),
            v("0"),
        ],
        Vendor::Qsv => vec![
            v("-c:v"),
            name,
            v("-preset"),
            v("medium"),
            v("-global_quality"),
            q,
        ],
        Vendor::Amf => vec![
            v("-c:v"),
            name,
            v("-quality"),
            v("quality"),
            v("-rc"),
            v("cqp"),
            v("-qp_i"),
            q.clone(),
            v("-qp_p"),
            q.clone(),
            v("-qp_b"),
            q,
        ],
        Vendor::Cpu => vec![],
    }
}

/// Hardware bitrate (target-size) args, vendor-specific single-pass VBR.
fn hw_bitrate_args(venc: &VideoEnc, vk: i64) -> Vec<String> {
    let name = venc.name.to_string();
    let vb = format!("{vk}k");
    let mx = format!("{}k", (vk as f64 * 1.2).round() as i64);
    let bf = format!("{}k", vk * 2);
    let v = |s: &str| s.to_string();
    match venc.vendor {
        Vendor::Nvenc => vec![
            v("-c:v"),
            name,
            v("-preset"),
            v("p6"),
            v("-rc"),
            v("vbr"),
            v("-b:v"),
            vb,
            v("-maxrate"),
            mx,
            v("-bufsize"),
            bf,
            v("-multipass"),
            v("fullres"),
        ],
        Vendor::Qsv => vec![v("-c:v"), name, v("-b:v"), vb, v("-maxrate"), mx],
        Vendor::Amf => vec![
            v("-c:v"),
            name,
            v("-rc"),
            v("vbr_peak"),
            v("-b:v"),
            vb,
            v("-maxrate"),
            mx,
        ],
        Vendor::Cpu => vec![],
    }
}

// ---------------------------------------------------------------------------
// commands: ffmpeg availability + download
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct FfmpegStatus {
    present: bool,
    on_path: bool,
    ffmpeg: Option<String>,
}

// NOTE: heavy/blocking commands are `async` so they run on the async runtime's
// thread pool - sync Tauri commands run on the MAIN thread and freeze the UI.
#[tauri::command]
async fn check_ffmpeg(state: State<'_, AppState>) -> Result<FfmpegStatus, String> {
    let (m, p) = locate_tools();
    let from_path = m.as_deref() == Some(Path::new("ffmpeg"));
    *state.ffmpeg.lock().unwrap() = m.clone();
    *state.ffprobe.lock().unwrap() = p;
    Ok(FfmpegStatus {
        present: m.is_some(),
        on_path: from_path,
        ffmpeg: m.map(|p| p.to_string_lossy().to_string()),
    })
}

#[tauri::command]
fn download_ffmpeg(app: AppHandle) {
    std::thread::spawn(move || {
        if let Err(e) = do_download(&app) {
            let _ = app.emit(
                "setup",
                serde_json::json!({ "stage": "error", "message": e }),
            );
        }
    });
}

fn do_download(app: &AppHandle) -> Result<(), String> {
    let emit = |stage: &str, percent: f64, msg: &str| {
        let _ = app.emit(
            "setup",
            serde_json::json!({ "stage": stage, "percent": percent, "message": msg }),
        );
    };

    emit("download", 0.0, "Reaching out for FFmpeg…");
    let client = reqwest::blocking::Client::builder()
        .user_agent("Meowverter")
        .build()
        .map_err(|e| e.to_string())?;
    let mut resp = client
        .get(FFMPEG_URL)
        .send()
        .map_err(|e| format!("download failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("download failed: HTTP {}", resp.status()));
    }
    let total = resp.content_length().unwrap_or(0);

    let tmp = temp_path("meowverter_ffmpeg", "zip");
    let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 1 << 16];
    let mut done: u64 = 0;
    let mut last = Instant::now();
    loop {
        let n = resp.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        if last.elapsed() > Duration::from_millis(120) {
            let pct = if total > 0 {
                done as f64 / total as f64 * 100.0
            } else {
                0.0
            };
            emit(
                "download",
                pct,
                &format!("Downloading FFmpeg… {:.0} MB", done as f64 / 1_048_576.0),
            );
            last = Instant::now();
        }
    }
    drop(file);

    emit("extract", 100.0, "Unpacking…");
    let f = std::fs::File::open(&tmp).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(f).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(bin_dir()).map_err(|e| e.to_string())?;
    let mut found = 0;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().replace('\\', "/");
        if name.ends_with("/bin/ffmpeg.exe") || name.ends_with("/bin/ffprobe.exe") {
            let leaf = name.rsplit('/').next().unwrap();
            let out = bin_dir().join(leaf);
            let mut o = std::fs::File::create(&out).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut o).map_err(|e| e.to_string())?;
            found += 1;
        }
    }
    let _ = std::fs::remove_file(&tmp);
    if found < 2 {
        return Err("couldn't find ffmpeg/ffprobe inside the download".into());
    }

    let state = app.state::<AppState>();
    *state.ffmpeg.lock().unwrap() = Some(bin_dir().join("ffmpeg.exe"));
    *state.ffprobe.lock().unwrap() = Some(bin_dir().join("ffprobe.exe"));
    // remember which release we just installed, so the updater compares like-for-like
    if let Ok(marker) = latest_ffmpeg_marker() {
        let _ = std::fs::write(marker_path(), marker);
    }
    emit("done", 100.0, "FFmpeg ready ✨");
    Ok(())
}

// ---------------------------------------------------------------------------
// command: check for an FFmpeg update (BtbN rolling build)
// ---------------------------------------------------------------------------

/// A stable identifier for the latest BtbN build - the win64-gpl asset's upload
/// time. We store the one we installed and compare against this; the build's own
/// embedded date lags the publish time, which caused false "update" nags.
fn latest_ffmpeg_marker() -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("Meowverter")
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|e| e.to_string())?;
    let body = client
        .get("https://api.github.com/repos/BtbN/FFmpeg-Builds/releases/tags/latest")
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| e.to_string())?
        .text()
        .map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    if let Some(assets) = v.get("assets").and_then(|a| a.as_array()) {
        for a in assets {
            if a.get("name").and_then(|n| n.as_str()) == Some("ffmpeg-master-latest-win64-gpl.zip")
            {
                if let Some(u) = a.get("updated_at").and_then(|x| x.as_str()) {
                    return Ok(u.to_string());
                }
            }
        }
    }
    Ok(v.get("published_at")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string())
}

fn marker_path() -> PathBuf {
    bin_dir().join("ffmpeg_release.txt")
}

#[derive(Serialize)]
struct UpdateInfo {
    available: bool,
    current: String,
    latest: String,
}

#[tauri::command]
async fn check_ffmpeg_update() -> Result<UpdateInfo, String> {
    let latest = latest_ffmpeg_marker()?;
    let stored = std::fs::read_to_string(marker_path())
        .unwrap_or_default()
        .trim()
        .to_string();
    if stored.is_empty() {
        // pre-installed / first run: adopt the current build as the baseline, don't nag
        let _ = std::fs::write(marker_path(), &latest);
        return Ok(UpdateInfo {
            available: false,
            current: latest.clone(),
            latest,
        });
    }
    let available = !latest.is_empty() && latest != stored;
    Ok(UpdateInfo {
        available,
        current: stored,
        latest,
    })
}

// ---------------------------------------------------------------------------
// command: probe media
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct MediaInfo {
    duration: f64,
    width: u32,
    height: u32,
    vcodec: String,
    acodec: String,
    fps: f64,
    has_audio: bool,
    has_video: bool,
    size_bytes: u64,
}

fn parse_fps(s: &str) -> f64 {
    if let Some((a, b)) = s.split_once('/') {
        let a: f64 = a.parse().unwrap_or(0.0);
        let b: f64 = b.parse().unwrap_or(1.0);
        if b != 0.0 {
            return a / b;
        }
    }
    s.parse().unwrap_or(0.0)
}

#[tauri::command]
async fn probe(state: State<'_, AppState>, path: String) -> Result<MediaInfo, String> {
    let ffprobe = stored_ffprobe(&state)?;
    let out = new_cmd(&ffprobe)
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(&path)
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "ffprobe failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;

    let mut info = MediaInfo {
        duration: 0.0,
        width: 0,
        height: 0,
        vcodec: String::new(),
        acodec: String::new(),
        fps: 0.0,
        has_audio: false,
        has_video: false,
        size_bytes: 0,
    };

    if let Some(fmt) = v.get("format") {
        info.duration = fmt
            .get("duration")
            .and_then(|d| d.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);
        info.size_bytes = fmt
            .get("size")
            .and_then(|d| d.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
    }

    if let Some(streams) = v.get("streams").and_then(|s| s.as_array()) {
        for s in streams {
            match s.get("codec_type").and_then(|c| c.as_str()) {
                Some("video") if !info.has_video => {
                    info.has_video = true;
                    info.width = s.get("width").and_then(|w| w.as_u64()).unwrap_or(0) as u32;
                    info.height = s.get("height").and_then(|h| h.as_u64()).unwrap_or(0) as u32;
                    info.vcodec = s
                        .get("codec_name")
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .to_string();
                    let fr = s
                        .get("avg_frame_rate")
                        .and_then(|c| c.as_str())
                        .filter(|x| *x != "0/0")
                        .or_else(|| s.get("r_frame_rate").and_then(|c| c.as_str()))
                        .unwrap_or("0/1");
                    info.fps = parse_fps(fr);
                }
                Some("audio") if !info.has_audio => {
                    info.has_audio = true;
                    info.acodec = s
                        .get("codec_name")
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .to_string();
                }
                _ => {}
            }
        }
    }
    Ok(info)
}

// ---------------------------------------------------------------------------
// command: thumbnail (single frame at a timestamp, for the trim preview)
// ---------------------------------------------------------------------------

#[tauri::command]
async fn thumbnail(state: State<'_, AppState>, path: String, time: f64) -> Result<String, String> {
    let ff = stored_ffmpeg(&state)?;
    let t = time.max(0.0);
    let out = new_cmd(&ff)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-ss",
            &format!("{t:.3}"),
            "-i",
            &path,
            "-an",
            "-sn",
            "-frames:v",
            "1",
            "-vf",
            "scale=480:-2:flags=lanczos",
            "-q:v",
            "5",
            "-f",
            "image2",
            "-c:v",
            "mjpeg",
            "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() || out.stdout.is_empty() {
        return Err(format!(
            "thumbnail failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&out.stdout);
    Ok(format!("data:image/jpeg;base64,{b64}"))
}

// ---------------------------------------------------------------------------
// command: estimate GIF size (encode a short sample, scale by frame count)
// ---------------------------------------------------------------------------

/// GIF filtergraph. Quality 1..5 trades palette colours + dithering for file size.
fn gif_vf(fps: f64, h: i32, quality: i64) -> String {
    let q = if quality == 0 { 3 } else { quality.clamp(1, 5) };
    let colors = match q {
        1 => 64,
        2 => 128,
        3 => 192,
        4 => 256,
        _ => 256,
    };
    let dither = match q {
        1 => "none",
        2 => "bayer:bayer_scale=4",
        3 => "bayer:bayer_scale=3",
        4 => "bayer:bayer_scale=2",
        _ => "sierra2_4a",
    };
    format!(
        "fps={fps},scale=-2:{h}:flags=lanczos,split[a][b];[a]palettegen=max_colors={colors}:stats_mode=diff[p];[b][p]paletteuse=dither={dither}"
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GifEstOpts {
    input: String,
    #[serde(default)]
    resolution: String,
    #[serde(default)]
    fps: f64,
    #[serde(default)]
    start: f64,
    #[serde(default)]
    duration: f64,
    #[serde(default)]
    gif_quality: i64,
}

#[tauri::command]
async fn estimate_gif(state: State<'_, AppState>, opts: GifEstOpts) -> Result<u64, String> {
    let ff = stored_ffmpeg(&state)?;
    let fps = if opts.fps > 0.0 { opts.fps } else { 15.0 };
    let full = opts.duration.max(0.1);
    let sample = full.min(2.0);
    let h = match opts.resolution.as_str() {
        "source" | "" => 360,
        q => q.parse::<i32>().unwrap_or(360).min(480),
    };
    let vf = gif_vf(fps, h, opts.gif_quality);
    let tmp = temp_path("meowverter_gifest", "gif");
    let _ = std::fs::remove_file(&tmp);
    let out = new_cmd(&ff)
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-ss",
            &format!("{:.3}", opts.start.max(0.0)),
            "-t",
            &format!("{:.3}", sample),
            "-i",
            &opts.input,
            "-vf",
            &vf,
            "-loop",
            "0",
            "-an",
        ])
        .arg(&tmp)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "gif estimate failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let sample_bytes = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
    let _ = std::fs::remove_file(&tmp);
    if sample_bytes == 0 {
        return Err("gif estimate produced no output".into());
    }
    // subtract the fixed header/palette overhead before scaling, add it back once
    let overhead: u64 = 1024;
    let per = sample_bytes.saturating_sub(overhead) as f64;
    let est = (per * (full / sample)).round() as u64 + overhead;
    Ok(est)
}

// ---------------------------------------------------------------------------
// command: VMAF quality score (output vs source)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VmafOpts {
    reference: String, // source file
    distorted: String, // converted output
    #[serde(default)]
    ref_start: f64, // where the output begins in the source (trim start)
    #[serde(default)]
    seconds: f64, // sample length cap
}

#[tauri::command]
async fn vmaf(state: State<'_, AppState>, opts: VmafOpts) -> Result<f64, String> {
    let ff = stored_ffmpeg(&state)?;
    let fp = stored_ffprobe(&state)?;
    // output dimensions - reference is scaled to match so VMAF can compare frame-for-frame
    let dout = new_cmd(&fp)
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "csv=p=0",
        ])
        .arg(&opts.distorted)
        .stderr(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    let dims = String::from_utf8_lossy(&dout.stdout);
    let mut it = dims.trim().split(',');
    let w: i64 = it.next().and_then(|x| x.trim().parse().ok()).unwrap_or(0);
    let h: i64 = it.next().and_then(|x| x.trim().parse().ok()).unwrap_or(0);
    if w == 0 || h == 0 {
        return Err("couldn't read output dimensions".into());
    }
    let sample = if opts.seconds > 0.0 {
        opts.seconds
    } else {
        20.0
    };
    let lavfi = format!(
        "[0:v]setpts=PTS-STARTPTS[d];[1:v]scale={w}:{h}:flags=bicubic,setpts=PTS-STARTPTS[r];[d][r]libvmaf=n_threads=16"
    );
    let out = new_cmd(&ff)
        .args([
            "-hide_banner",
            "-ss",
            "0",
            "-t",
            &format!("{sample:.3}"),
            "-i",
        ])
        .arg(&opts.distorted)
        .args([
            "-ss",
            &format!("{:.3}", opts.ref_start.max(0.0)),
            "-t",
            &format!("{sample:.3}"),
            "-i",
        ])
        .arg(&opts.reference)
        .args(["-lavfi", &lavfi, "-f", "null", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    let s = String::from_utf8_lossy(&out.stderr);
    for line in s.lines() {
        if let Some(p) = line.split("VMAF score:").nth(1) {
            if let Ok(v) = p.trim().parse::<f64>() {
                return Ok(v);
            }
        }
    }
    Err("VMAF couldn't be computed for this file".into())
}

// ---------------------------------------------------------------------------
// command: convert
// ---------------------------------------------------------------------------

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ConvertOpts {
    input: String,
    output: String,
    mode: String, // "video" | "audio" | "gif"
    #[serde(default)]
    resolution: String, // "source" | "2160" | "1440" | "1080" | "720" | "480" | "360"
    #[serde(default)]
    format: String, // "mp4_h264" | "mp4_h265" | "webm" | "mkv"
    #[serde(default)]
    quality: String, // "high" | "balanced" | "small"
    #[serde(default)]
    target_size_mb: Option<f64>,
    #[serde(default)]
    trim_start: Option<f64>,
    #[serde(default)]
    trim_end: Option<f64>,
    #[serde(default)]
    fps: Option<f64>,
    #[serde(default)]
    audio_format: String, // "mp3" | "m4a" | "wav"
    #[serde(default)]
    gif_quality: i64, // 1..5 (palette colours + dither)
    #[serde(default)]
    total_duration: f64, // duration (after trim) for progress %, seconds
    #[serde(default)]
    silent: bool, // suppress the per-file done notification (batch)
    #[serde(default)]
    delete_original: bool, // on success: original -> Recycle Bin, output takes its name
}

struct Codec {
    venc: &'static str,
    aenc: &'static str,
    pix: bool, // force yuv420p
    faststart: bool,
    pass_fmt: &'static str,
    extra: Vec<&'static str>,
}

fn codec_for(format: &str) -> Codec {
    match format {
        "mp4_h265" => Codec {
            venc: "libx265",
            aenc: "aac",
            pix: true,
            faststart: true,
            pass_fmt: "mp4",
            extra: vec!["-tag:v", "hvc1"],
        },
        "mp4_av1" => Codec {
            venc: "libsvtav1",
            aenc: "aac",
            pix: true,
            faststart: true,
            pass_fmt: "mp4",
            extra: vec![],
        },
        "webm" => Codec {
            venc: "libvpx-vp9",
            aenc: "libopus",
            pix: false,
            faststart: false,
            pass_fmt: "webm",
            // row-mt + a practical speed level - VP9 defaults to cpu-used 0 (glacial)
            extra: vec!["-row-mt", "1", "-deadline", "good", "-cpu-used", "4"],
        },
        "mkv" => Codec {
            venc: "libx265",
            aenc: "aac",
            pix: true,
            faststart: false,
            pass_fmt: "matroska",
            extra: vec![],
        },
        _ => Codec {
            venc: "libx264",
            aenc: "aac",
            pix: true,
            faststart: true,
            pass_fmt: "mp4",
            extra: vec![],
        },
    }
}

fn crf_for(format: &str, quality: &str) -> &'static str {
    match (format, quality) {
        ("webm", "high") => "24",
        ("webm", "small") => "37",
        ("webm", _) => "31",
        ("mp4_h265", "high") => "21",
        ("mp4_h265", "small") => "30",
        ("mp4_h265", _) => "26",
        ("mp4_av1", "high") => "28",
        ("mp4_av1", "small") => "45",
        ("mp4_av1", _) => "35",
        (_, "high") => "18",
        (_, "small") => "27",
        (_, _) => "22",
    }
}

fn input_args(o: &ConvertOpts, gpu_decode: bool) -> Vec<String> {
    let mut a: Vec<String> = vec!["-y".into(), "-hide_banner".into(), "-nostdin".into()];
    if gpu_decode {
        a.extend(
            ["-hwaccel", "cuda", "-hwaccel_output_format", "cuda"]
                .iter()
                .map(|s| s.to_string()),
        );
    }
    if let Some(s) = o.trim_start {
        if s > 0.0 {
            a.push("-ss".into());
            a.push(format!("{s:.3}"));
        }
    }
    a.push("-i".into());
    a.push(o.input.clone());
    match (o.trim_start, o.trim_end) {
        (Some(s), Some(e)) if e > s => {
            a.push("-t".into());
            a.push(format!("{:.3}", e - s));
        }
        (None, Some(e)) if e > 0.0 => {
            a.push("-t".into());
            a.push(format!("{e:.3}"));
        }
        _ => {}
    }
    a
}

fn scale_filter(res: &str) -> Option<String> {
    scale_filter_named(res, "scale")
}
fn scale_filter_named(res: &str, name: &str) -> Option<String> {
    if res.is_empty() || res == "source" {
        None
    } else {
        Some(format!("{name}=-2:{res}"))
    }
}

/// Build one or more passes (each a full ffmpeg arg vector).
fn build_passes(
    o: &ConvertOpts,
    venc: VideoEnc,
    gpu_decode: bool,
) -> Result<Vec<Vec<String>>, String> {
    let progress = ["-progress", "pipe:1", "-nostats"];

    match o.mode.as_str() {
        "audio" => {
            let mut a = input_args(o, false);
            a.push("-vn".into());
            match o.audio_format.as_str() {
                "wav" => {
                    a.push("-c:a".into());
                    a.push("pcm_s16le".into());
                }
                "m4a" => {
                    a.extend(
                        ["-c:a", "aac", "-b:a", "192k"]
                            .iter()
                            .map(|s| s.to_string()),
                    );
                }
                _ => {
                    a.extend(
                        ["-c:a", "libmp3lame", "-b:a", "192k"]
                            .iter()
                            .map(|s| s.to_string()),
                    );
                }
            }
            a.extend(progress.iter().map(|s| s.to_string()));
            a.push(o.output.clone());
            Ok(vec![a])
        }

        "gif" => {
            let fps = o.fps.unwrap_or(15.0).clamp(5.0, 50.0);
            let h = match o.resolution.as_str() {
                "source" | "" => 360,
                other => other.parse::<i32>().unwrap_or(360).min(480),
            };
            let vf = gif_vf(fps, h, o.gif_quality);
            let mut a = input_args(o, false);
            a.push("-vf".into());
            a.push(vf);
            a.push("-loop".into());
            a.push("0".into());
            a.push("-an".into());
            a.extend(progress.iter().map(|s| s.to_string()));
            a.push(o.output.clone());
            Ok(vec![a])
        }

        _ => {
            // video
            let c = codec_for(&o.format);
            let vf = scale_filter(&o.resolution);
            let hw = venc.vendor != Vendor::Cpu;
            // full NVDEC -> scale_cuda -> NVENC pipeline (NVIDIA only)
            let use_cuda = venc.vendor == Vendor::Nvenc && gpu_decode;
            let svtav1 = venc.name == "libsvtav1";

            if let Some(mb) = o.target_size_mb {
                // compress-to-size with a computed bitrate
                let dur = o.total_duration.max(0.1);
                let audio_kbps = if c.aenc == "libopus" { 96.0 } else { 128.0 };
                let total_kbps = mb * 8192.0 / dur;
                let video_kbps = (total_kbps - audio_kbps).max(80.0);
                let vk = video_kbps.round() as i64;
                let vb = format!("{vk}k");

                if hw {
                    // single-pass hardware VBR to the target bitrate
                    let mut a = input_args(o, use_cuda);
                    let sc = if use_cuda {
                        scale_filter_named(&o.resolution, "scale_cuda")
                    } else {
                        vf.clone()
                    };
                    if let Some(f) = &sc {
                        a.push("-vf".into());
                        a.push(f.clone());
                    }
                    a.extend(hw_bitrate_args(&venc, vk));
                    a.extend(c.extra.iter().map(|s| s.to_string())); // container tag (hvc1 for hevc)
                    if !use_cuda {
                        a.push("-pix_fmt".into());
                        a.push("yuv420p".into());
                    }
                    a.push("-c:a".into());
                    a.push(c.aenc.into());
                    a.push("-b:a".into());
                    a.push(format!("{}k", audio_kbps as i64));
                    if c.faststart {
                        a.extend(["-movflags", "+faststart"].iter().map(|s| s.to_string()));
                    }
                    a.extend(progress.iter().map(|s| s.to_string()));
                    a.push(o.output.clone());
                    return Ok(vec![a]);
                }

                if svtav1 {
                    // CPU AV1: single-pass VBR (svtav1 two-pass is finicky)
                    let mut a = input_args(o, false);
                    if let Some(f) = &vf {
                        a.push("-vf".into());
                        a.push(f.clone());
                    }
                    a.extend(
                        ["-c:v", "libsvtav1", "-preset", "8", "-b:v"]
                            .iter()
                            .map(|s| s.to_string()),
                    );
                    a.push(vb);
                    a.extend(
                        ["-pix_fmt", "yuv420p", "-c:a"]
                            .iter()
                            .map(|s| s.to_string()),
                    );
                    a.push(c.aenc.into());
                    a.push("-b:a".into());
                    a.push(format!("{}k", audio_kbps as i64));
                    a.extend(["-movflags", "+faststart"].iter().map(|s| s.to_string()));
                    a.extend(progress.iter().map(|s| s.to_string()));
                    a.push(o.output.clone());
                    return Ok(vec![a]);
                }

                let log =
                    std::env::temp_dir().join(format!("meowverter_pass_{}", std::process::id()));
                let logf = log.to_string_lossy().to_string();

                let mut p1 = input_args(o, false);
                if let Some(f) = &vf {
                    p1.push("-vf".into());
                    p1.push(f.clone());
                }
                p1.push("-c:v".into());
                p1.push(c.venc.into());
                p1.push("-b:v".into());
                p1.push(vb.clone());
                p1.extend(c.extra.iter().map(|s| s.to_string()));
                p1.extend(["-pass", "1", "-passlogfile"].iter().map(|s| s.to_string()));
                p1.push(logf.clone());
                p1.push("-an".into());
                p1.push("-f".into());
                p1.push(c.pass_fmt.into());
                p1.extend(progress.iter().map(|s| s.to_string()));
                p1.push("NUL".into());

                let mut p2 = input_args(o, false);
                if let Some(f) = &vf {
                    p2.push("-vf".into());
                    p2.push(f.clone());
                }
                p2.push("-c:v".into());
                p2.push(c.venc.into());
                p2.push("-b:v".into());
                p2.push(vb);
                if c.pix {
                    p2.push("-pix_fmt".into());
                    p2.push("yuv420p".into());
                }
                p2.extend(c.extra.iter().map(|s| s.to_string()));
                p2.extend(["-pass", "2", "-passlogfile"].iter().map(|s| s.to_string()));
                p2.push(logf);
                p2.push("-c:a".into());
                p2.push(c.aenc.into());
                p2.push("-b:a".into());
                p2.push(format!("{}k", audio_kbps as i64));
                if c.faststart {
                    p2.push("-movflags".into());
                    p2.push("+faststart".into());
                }
                p2.extend(progress.iter().map(|s| s.to_string()));
                p2.push(o.output.clone());

                Ok(vec![p1, p2])
            } else {
                if hw {
                    // hardware constant-quality encode
                    let mut a = input_args(o, use_cuda);
                    let sc = if use_cuda {
                        scale_filter_named(&o.resolution, "scale_cuda")
                    } else {
                        vf.clone()
                    };
                    if let Some(f) = &sc {
                        a.push("-vf".into());
                        a.push(f.clone());
                    }
                    a.extend(hw_quality_args(&venc, &o.quality));
                    a.extend(c.extra.iter().map(|s| s.to_string())); // container tag (hvc1 for hevc)
                    if !use_cuda {
                        a.push("-pix_fmt".into());
                        a.push("yuv420p".into());
                    }
                    a.push("-c:a".into());
                    a.push(c.aenc.into());
                    a.push("-b:a".into());
                    a.push("160k".into());
                    if c.faststart {
                        a.extend(["-movflags", "+faststart"].iter().map(|s| s.to_string()));
                    }
                    a.extend(progress.iter().map(|s| s.to_string()));
                    a.push(o.output.clone());
                    return Ok(vec![a]);
                }
                // CPU quality (CRF) single pass
                let crf = crf_for(&o.format, &o.quality);
                let mut a = input_args(o, false);
                if let Some(f) = &vf {
                    a.push("-vf".into());
                    a.push(f.clone());
                }
                a.push("-c:v".into());
                a.push(c.venc.into());
                if c.venc == "libvpx-vp9" {
                    a.extend(["-b:v", "0"].iter().map(|s| s.to_string()));
                } else if svtav1 {
                    a.extend(["-preset", "8"].iter().map(|s| s.to_string()));
                } else {
                    a.extend(["-preset", "medium"].iter().map(|s| s.to_string()));
                }
                a.push("-crf".into());
                a.push(crf.into());
                if c.pix {
                    a.push("-pix_fmt".into());
                    a.push("yuv420p".into());
                }
                a.extend(c.extra.iter().map(|s| s.to_string()));
                a.push("-c:a".into());
                a.push(c.aenc.into());
                a.push("-b:a".into());
                a.push(if c.aenc == "libopus" {
                    "128k".into()
                } else {
                    "160k".into()
                });
                if c.faststart {
                    a.push("-movflags".into());
                    a.push("+faststart".into());
                }
                a.extend(progress.iter().map(|s| s.to_string()));
                a.push(o.output.clone());
                Ok(vec![a])
            }
        }
    }
}

#[tauri::command]
fn cancel_convert(state: State<AppState>) {
    *state.cancel.lock().unwrap() = true;
    if let Some(child) = state.child.lock().unwrap().as_mut() {
        let _ = child.kill();
    }
}

fn available_output_path(requested: &str) -> String {
    let path = Path::new(requested);
    if !path.exists() {
        return requested.to_string();
    }
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    for n in 2..10_000 {
        let name = if ext.is_empty() {
            format!("{stem}_{n}")
        } else {
            format!("{stem}_{n}.{ext}")
        };
        let candidate = parent.join(name);
        if !candidate.exists() {
            return candidate.to_string_lossy().to_string();
        }
    }
    requested.to_string()
}

fn remove_partial_output(opts: &ConvertOpts) {
    let _ = std::fs::remove_file(&opts.output);
}

#[tauri::command]
async fn start_convert(
    app: AppHandle,
    state: State<'_, AppState>,
    mut opts: ConvertOpts,
) -> Result<(), String> {
    let ffmpeg = stored_ffmpeg(&state)?;
    if Path::new(&opts.input) == Path::new(&opts.output) {
        return Err("The output file must be different from the original.".into());
    }
    opts.output = available_output_path(&opts.output);
    let dur = opts.total_duration.max(0.1);

    // Pick the fastest working encoder for this machine, with a fallback chain:
    // hardware + GPU decode -> hardware + CPU decode (NVIDIA) -> CPU encoder.
    // If a "supported" hardware encoder still errors mid-encode, we step down.
    let chosen = pick_encoder(&state, &ffmpeg, &opts.format);
    let cpu = cpu_encoder(&opts.format);
    let attempts: Vec<(VideoEnc, bool)> = if opts.mode != "video" {
        vec![(cpu, false)] // audio/gif don't use the video encoder
    } else if chosen.vendor == Vendor::Nvenc {
        vec![(chosen, true), (chosen, false), (cpu, false)]
    } else if chosen.vendor != Vendor::Cpu {
        vec![(chosen, false), (cpu, false)]
    } else {
        vec![(cpu, false)]
    };

    // validate the first attempt builds before spawning
    build_passes(&opts, attempts[0].0, attempts[0].1)?;
    claim_job(&state)?;
    *state.cancel.lock().unwrap() = false;

    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let _active_job = ActiveJob(&state);
        let mut attempt = 0;
        loop {
            let (venc, gpu_decode) = attempts[attempt];
            let passes = match build_passes(&opts, venc, gpu_decode) {
                Ok(p) => p,
                Err(e) => {
                    remove_partial_output(&opts);
                    let _ = app.emit(
                        "convert",
                        serde_json::json!({ "stage": "error", "message": e }),
                    );
                    return;
                }
            };
            let mut idx = 0;
            let mut retry = false;
            while idx < passes.len() {
                if *state.cancel.lock().unwrap() {
                    remove_partial_output(&opts);
                    let _ = app.emit("convert", serde_json::json!({ "stage": "cancelled" }));
                    return;
                }
                match run_pass(&app, &state, &ffmpeg, &passes[idx], dur, idx, passes.len()) {
                    Ok(true) => {
                        idx += 1;
                    } // finished this pass
                    Ok(false) => {
                        remove_partial_output(&opts);
                        let _ = app.emit("convert", serde_json::json!({ "stage": "cancelled" }));
                        return;
                    }
                    Err(e) => {
                        // a hardware encoder that isn't truly usable fails here ->
                        // step down to the next attempt (which ends at the CPU encoder)
                        if idx == 0
                            && attempt + 1 < attempts.len()
                            && !*state.cancel.lock().unwrap()
                        {
                            attempt += 1;
                            retry = true;
                            break;
                        }
                        remove_partial_output(&opts);
                        let _ = app.emit(
                            "convert",
                            serde_json::json!({ "stage": "error", "message": e }),
                        );
                        return;
                    }
                }
            }
            if retry {
                continue;
            }
            break;
        }
        let out_size = std::fs::metadata(&opts.output)
            .map(|m| m.len())
            .unwrap_or(0);
        if out_size == 0 {
            remove_partial_output(&opts);
            let _ = app.emit(
                "convert",
                serde_json::json!({
                    "stage": "error",
                    "message": "FFmpeg finished without creating a usable output file."
                }),
            );
            return;
        }
        let final_out = finish_delete_original(&opts, out_size);
        let _ = app.emit(
            "convert",
            serde_json::json!({ "stage": "done", "output": final_out, "outputSize": out_size }),
        );
        if !opts.silent {
            notify(&app, "Conversion done", &leaf(&final_out));
        }
    });

    Ok(())
}

/// "Delete original" epilogue: after a VERIFIED successful convert, move the
/// source to the Recycle Bin (recoverable, never a hard delete). The converted
/// file KEEPS its "_Meowverter" name so you can always tell which files are
/// converted. Returns the (unchanged) output path.
fn finish_delete_original(opts: &ConvertOpts, out_size: u64) -> String {
    if opts.delete_original && out_size > 0 && opts.input != opts.output {
        let _ = trash::delete(&opts.input);
    }
    opts.output.clone()
}

/// File name only, for notifications.
fn leaf(path: &str) -> String {
    path.replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .to_string()
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app.notification().builder().title(title).body(body).show();
}

/// Runs one ffmpeg pass, streaming progress. Ok(true)=done, Ok(false)=cancelled.
fn run_pass(
    app: &AppHandle,
    state: &AppState,
    ffmpeg: &Path,
    args: &[String],
    dur: f64,
    pass_index: usize,
    total_passes: usize,
) -> Result<bool, String> {
    let mut child = new_cmd(ffmpeg)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to launch ffmpeg: {e}"))?;

    let stdout = child.stdout.take().unwrap();

    // drain stderr on a side thread, keeping the tail for error messages
    let stderr = child.stderr.take().unwrap();
    let err_tail = std::sync::Arc::new(Mutex::new(String::new()));
    let err_tail2 = err_tail.clone();
    let err_thread = std::thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut s = String::new();
        let _ = reader.read_to_string(&mut s);
        let tail: String = s
            .chars()
            .rev()
            .take(4000)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        *err_tail2.lock().unwrap() = tail;
    });

    *state.child.lock().unwrap() = Some(child);

    let reader = BufReader::new(stdout);
    let mut last = Instant::now();
    let mut speed = String::new();
    let mut fps = String::new();
    for line in reader.lines() {
        if *state.cancel.lock().unwrap() {
            if let Some(c) = state.child.lock().unwrap().as_mut() {
                let _ = c.kill();
            }
            break;
        }
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if let Some(v) = line.strip_prefix("out_time_us=") {
            if let Ok(us) = v.trim().parse::<f64>() {
                let secs = us / 1_000_000.0;
                let frac = (secs / dur).clamp(0.0, 1.0);
                let overall = (pass_index as f64 + frac) / total_passes as f64 * 100.0;
                if last.elapsed() > Duration::from_millis(90) {
                    let _ = app.emit(
                        "convert",
                        serde_json::json!({
                            "stage": "progress",
                            "percent": overall,
                            "pass": pass_index + 1,
                            "passes": total_passes,
                            "speed": speed,
                            "fps": fps,
                        }),
                    );
                    last = Instant::now();
                }
            }
        } else if let Some(v) = line.strip_prefix("speed=") {
            speed = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("fps=") {
            fps = v.trim().to_string();
        }
    }

    let status = {
        let mut guard = state.child.lock().unwrap();
        match guard.as_mut() {
            Some(c) => c.wait().map_err(|e| e.to_string())?,
            None => return Ok(false),
        }
    };
    *state.child.lock().unwrap() = None;
    let _ = err_thread.join();

    if *state.cancel.lock().unwrap() {
        return Ok(false);
    }
    if !status.success() {
        let tail = err_tail.lock().unwrap().clone();
        let msg = tail
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("ffmpeg failed")
            .to_string();
        return Err(msg);
    }
    Ok(true)
}

// ---------------------------------------------------------------------------
// commands: file dialogs + reveal
// ---------------------------------------------------------------------------

const MEDIA_EXTS: &[&str] = &[
    "mp4", "mkv", "mov", "avi", "webm", "flv", "wmv", "m4v", "ts", "mpg", "mpeg", "mp3", "wav",
    "flac", "aac", "ogg", "m4a", "gif",
];

#[tauri::command]
async fn pick_inputs() -> Vec<String> {
    rfd::FileDialog::new()
        .add_filter("Media", MEDIA_EXTS)
        .add_filter("All files", &["*"])
        .pick_files()
        .map(|v| {
            v.into_iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default()
}

#[tauri::command]
async fn pick_folder() -> Option<String> {
    rfd::FileDialog::new()
        .pick_folder()
        .map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
async fn pick_output(default_name: String, ext: String) -> Option<String> {
    rfd::FileDialog::new()
        .set_file_name(&default_name)
        .add_filter(ext.to_uppercase(), &[ext.as_str()])
        .save_file()
        .map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
fn notify_done(app: AppHandle, title: String, body: String) {
    notify(&app, &title, &body);
}

#[tauri::command]
fn reveal(path: String) {
    let p = PathBuf::from(&path);
    #[cfg(windows)]
    {
        let _ = new_cmd("explorer").arg("/select,").arg(&p).spawn();
    }
    #[cfg(not(windows))]
    {
        if let Some(dir) = p.parent() {
            let _ = new_cmd("xdg-open").arg(dir).spawn();
        }
    }
}

// ---------------------------------------------------------------------------
// commands: YouTube download (yt-dlp)
// ---------------------------------------------------------------------------

fn ytdlp_path() -> Option<PathBuf> {
    let local = bin_dir().join("yt-dlp.exe");
    if local.exists() {
        Some(local)
    } else {
        None
    }
}

fn http_download(url: &str, dest: &Path) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("Meowverter")
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|e| e.to_string())?;
    let mut resp = client.get(url).send().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let mut f = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    std::io::copy(&mut resp, &mut f).map_err(|e| e.to_string())?;
    Ok(())
}

fn install_ytdlp() -> Result<PathBuf, String> {
    std::fs::create_dir_all(bin_dir()).map_err(|e| e.to_string())?;
    let dest = bin_dir().join("yt-dlp.exe");
    let tmp = temp_path("meowverter_ytdlp", "exe");
    if let Err(e) = http_download(YTDLP_URL, &tmp) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("couldn't download yt-dlp: {e}"));
    }
    if let Err(e) = std::fs::rename(&tmp, &dest) {
        if dest.exists() {
            let _ = std::fs::remove_file(&tmp);
        } else {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("couldn't install yt-dlp: {e}"));
        }
    }
    Ok(dest)
}

fn ensure_deno() -> Result<PathBuf, String> {
    let dest = bin_dir().join("deno.exe");
    if dest.exists() {
        return Ok(dest);
    }
    std::fs::create_dir_all(bin_dir()).map_err(|e| e.to_string())?;
    let archive_path = temp_path("meowverter_deno", "zip");
    if let Err(e) = http_download(DENO_URL, &archive_path) {
        let _ = std::fs::remove_file(&archive_path);
        return Err(format!("couldn't download the YouTube runtime: {e}"));
    }

    let extracted = temp_path("meowverter_deno", "exe");
    let result = (|| -> Result<(), String> {
        let file = std::fs::File::open(&archive_path).map_err(|e| e.to_string())?;
        let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
        let mut found = false;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
            let name = entry.name().replace('\\', "/");
            if name == "deno.exe" || name.ends_with("/deno.exe") {
                let mut out = std::fs::File::create(&extracted).map_err(|e| e.to_string())?;
                std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
                found = true;
                break;
            }
        }
        if !found {
            return Err("deno.exe wasn't present in its download".into());
        }
        Ok(())
    })();
    let _ = std::fs::remove_file(&archive_path);
    if let Err(e) = result {
        let _ = std::fs::remove_file(&extracted);
        return Err(format!("couldn't install the YouTube runtime: {e}"));
    }
    if let Err(e) = std::fs::rename(&extracted, &dest) {
        if dest.exists() {
            let _ = std::fs::remove_file(&extracted);
        } else {
            let _ = std::fs::remove_file(&extracted);
            return Err(format!("couldn't install the YouTube runtime: {e}"));
        }
    }
    Ok(dest)
}

fn ensure_download_tools(state: &AppState) -> Result<PathBuf, String> {
    let _setup = state.tool_setup.lock().unwrap();
    let yt = match ytdlp_path() {
        Some(path) => path,
        None => install_ytdlp()?,
    };
    ensure_deno()?;
    Ok(yt)
}

fn parse_percent(line: &str) -> Option<f64> {
    for tok in line.split_whitespace() {
        if let Some(num) = tok.strip_suffix('%') {
            if let Ok(p) = num.parse::<f64>() {
                return Some(p);
            }
        }
    }
    None
}

#[derive(Serialize)]
struct YtInfo {
    title: String,
    duration: f64,
    thumbnail: String,
    width: u32,
    height: u32,
    sizes: std::collections::HashMap<String, u64>,
}

/// Approximate download size per quality (best video ≤ cap + best audio) straight
/// from the info JSON's formats - avoids extra yt-dlp calls per link, since rapid
/// repeat requests are exactly what trips YouTube's bot checks.
fn sizes_from_formats(v: &serde_json::Value) -> std::collections::HashMap<String, u64> {
    let mut out = std::collections::HashMap::new();
    let formats = match v.get("formats").and_then(|f| f.as_array()) {
        Some(f) => f,
        None => return out,
    };
    let fsize = |f: &serde_json::Value| -> u64 {
        f.get("filesize")
            .and_then(|x| x.as_u64())
            .or_else(|| f.get("filesize_approx").and_then(|x| x.as_u64()))
            .unwrap_or(0)
    };
    // best audio-only track
    let mut audio: (f64, u64) = (0.0, 0);
    for f in formats {
        let vc = f.get("vcodec").and_then(|x| x.as_str()).unwrap_or("none");
        let ac = f.get("acodec").and_then(|x| x.as_str()).unwrap_or("none");
        if vc == "none" && ac != "none" {
            let tbr = f.get("tbr").and_then(|x| x.as_f64()).unwrap_or(0.0);
            let s = fsize(f);
            if s > 0 && tbr >= audio.0 {
                audio = (tbr, s);
            }
        }
    }
    for (key, cap) in [
        ("best", u64::MAX),
        ("2160", 2160),
        ("1440", 1440),
        ("1080", 1080),
        ("720", 720),
        ("480", 480),
        ("360", 360),
    ] {
        let mut best: (u64, f64, u64) = (0, 0.0, 0); // (height, tbr, size)
        for f in formats {
            let vc = f.get("vcodec").and_then(|x| x.as_str()).unwrap_or("none");
            if vc == "none" {
                continue;
            }
            let h = f.get("height").and_then(|x| x.as_u64()).unwrap_or(0);
            if h == 0 || h > cap {
                continue;
            }
            let tbr = f.get("tbr").and_then(|x| x.as_f64()).unwrap_or(0.0);
            let s = fsize(f);
            if s == 0 {
                continue;
            }
            if h > best.0 || (h == best.0 && tbr > best.1) {
                best = (h, tbr, s);
            }
        }
        if best.2 > 0 {
            out.insert(key.to_string(), best.2 + audio.1);
        }
    }
    out
}

/// Pull the content of a `<meta property="PROP" content="...">` tag.
fn meta_content(html: &str, prop: &str) -> Option<String> {
    let needle = format!("property=\"{prop}\"");
    let i = html.find(&needle)?;
    let after = &html[i + needle.len()..];
    let c = after.find("content=\"")?;
    let rest = &after[c + "content=\"".len()..];
    let end = rest.find('"')?;
    Some(html_unescape(rest[..end].trim()))
}

fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MusicResolved {
    youtube_url: String,
    title: String,
    thumbnail: String,
    duration: f64,
}

// Streaming pages serve a metadata-less JS stub to browser UAs but the full
// server-rendered page (with og: tags) to link-preview crawlers.
const CRAWLER_UA: &str =
    "facebookexternalhit/1.1 (+http://www.facebook.com/externalhit_uatext.php)";

fn http_text(url: &str) -> Result<String, String> {
    reqwest::blocking::Client::builder()
        .user_agent(CRAWLER_UA)
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?
        .get(url)
        .send()
        .and_then(|r| r.text())
        .map_err(|e| e.to_string())
}

fn deezer_track_id(url: &str) -> Option<String> {
    let after = url.split("/track/").nth(1)?;
    let id: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
    if id.is_empty() {
        None
    } else {
        Some(id)
    }
}

/// Streaming audio (Spotify, Apple Music, Tidal, Deezer) is DRM-locked and can't
/// be downloaded. Instead we read the track + artist from the public page/API
/// (no login involved) and find the same song on YouTube, which the downloader
/// then grabs. Returns the matching YouTube URL + display info.
#[tauri::command]
async fn resolve_music(state: State<'_, AppState>, url: String) -> Result<MusicResolved, String> {
    let u = url.to_lowercase();

    // 1) build a "artist track" search query + a clean display title + cover art
    let (query, title, thumbnail) = if u.contains("deezer.com") {
        // Deezer has a free public API - cleaner than scraping
        let id = deezer_track_id(&url).ok_or("Couldn't read that Deezer link.")?;
        let body = http_text(&format!("https://api.deezer.com/track/{id}"))?;
        let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        let track = v
            .get("title")
            .and_then(|x| x.as_str())
            .ok_or("Couldn't read that Deezer link.")?
            .to_string();
        let artist = v
            .get("artist")
            .and_then(|a| a.get("name"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let thumb = v
            .get("album")
            .and_then(|a| a.get("cover_medium"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        (
            format!("{artist} {track}"),
            format!("{artist} - {track}"),
            thumb,
        )
    } else {
        // scrape the og: tags (Spotify / Apple Music / Tidal)
        let html = http_text(&url)?;
        let og_title = meta_content(&html, "og:title").ok_or("Couldn't read that music link.")?;
        let thumb = meta_content(&html, "og:image").unwrap_or_default();
        if u.contains("music.apple.com") {
            // og:title = "Track by Artist on Apple Music" (note: nbsp between Apple & Music,
            // so split on " on Apple" to be safe)
            let core = og_title
                .split(" on Apple")
                .next()
                .unwrap_or(og_title.as_str())
                .trim();
            match core.rsplit_once(" by ") {
                Some((track, artist)) => (
                    format!("{artist} {track}"),
                    format!("{artist} - {track}"),
                    thumb,
                ),
                None => (core.to_string(), core.to_string(), thumb),
            }
        } else if u.contains("tidal.com") {
            // og:title = "Artist - Track" (already a good query + display)
            (og_title.clone(), og_title, thumb)
        } else {
            // Spotify: og:title = track, og:description = "Artist · Album · Song · Year"
            let desc = meta_content(&html, "og:description").unwrap_or_default();
            let artist = desc.split('·').next().unwrap_or("").trim().to_string();
            if artist.is_empty() {
                (og_title.clone(), og_title, thumb)
            } else {
                (
                    format!("{artist} {og_title}"),
                    format!("{artist} - {og_title}"),
                    thumb,
                )
            }
        }
    };

    // 2) find the best match on YouTube
    let yt = ensure_download_tools(&state)?;
    let bin = bin_dir();
    let path_env = format!(
        "{};{}",
        bin.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = new_cmd(&yt)
        .args([
            "--no-warnings",
            "--no-playlist",
            "--print",
            "%(webpage_url)s",
            "--print",
            "%(duration)s",
        ])
        .arg(format!("ytsearch1:{query}"))
        .env("PATH", path_env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err("Couldn't find that song to download.".into());
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let mut lines = s.lines().filter(|l| !l.trim().is_empty());
    let youtube_url = lines.next().unwrap_or("").trim().to_string();
    let duration = lines
        .next()
        .and_then(|d| d.trim().parse::<f64>().ok())
        .unwrap_or(0.0);
    if youtube_url.is_empty() {
        return Err("Couldn't find that song to download.".into());
    }
    Ok(MusicResolved {
        youtube_url,
        title,
        thumbnail,
        duration,
    })
}

#[tauri::command]
async fn youtube_info(state: State<'_, AppState>, url: String) -> Result<YtInfo, String> {
    let yt = ensure_download_tools(&state)?;
    let bin = bin_dir();
    let path_env = format!(
        "{};{}",
        bin.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = new_cmd(&yt)
        .args(["--dump-single-json", "--no-playlist", "--no-warnings"])
        .arg(&url)
        .env("PATH", path_env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("couldn't read that video")
            .to_string());
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    Ok(YtInfo {
        title: v
            .get("title")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        duration: v.get("duration").and_then(|x| x.as_f64()).unwrap_or(0.0),
        thumbnail: v
            .get("thumbnail")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        width: v.get("width").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
        height: v.get("height").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
        sizes: sizes_from_formats(&v),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct YtSizeOpts {
    url: String,
    #[serde(default)]
    quality: String,
}

/// Approx combined (video+audio) download size for the chosen quality, via a
/// no-download yt-dlp simulate. Returns 0 if YouTube doesn't report a size.
#[tauri::command]
async fn youtube_size(state: State<'_, AppState>, opts: YtSizeOpts) -> Result<u64, String> {
    let yt = ensure_download_tools(&state)?;
    let bin = bin_dir();
    let path_env = format!(
        "{};{}",
        bin.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    let h = match opts.quality.as_str() {
        "" | "best" | "source" => String::new(),
        q => format!("[height<={q}]"),
    };
    let sel = format!("bv*{h}+ba/b{h}");
    let out = new_cmd(&yt)
        .args([
            "--simulate",
            "--no-playlist",
            "--no-warnings",
            "-f",
            &sel,
            "--print",
            "%(filesize_approx)s",
        ])
        .arg(&opts.url)
        .env("PATH", path_env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr)
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("size lookup failed")
            .to_string());
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let bytes = s
        .lines()
        .find_map(|l| l.trim().parse::<u64>().ok())
        .unwrap_or(0);
    Ok(bytes)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct YtDownloadOpts {
    url: String,
    #[serde(default)]
    quality: String,
    #[serde(default)]
    format: String,
    #[serde(default)]
    mode: String,
    #[serde(default)]
    trim_start: Option<f64>,
    #[serde(default)]
    trim_end: Option<f64>,
}

#[tauri::command]
fn download_youtube(app: AppHandle, opts: YtDownloadOpts) -> Result<(), String> {
    let state = app.state::<AppState>();
    claim_job(&state)?;
    *state.cancel.lock().unwrap() = false;
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let _active_job = ActiveJob(&state);
        if let Err(e) = do_youtube(&app, &opts) {
            let _ = app.emit("yt", serde_json::json!({ "stage": "error", "message": e }));
        }
    });
    Ok(())
}

fn do_youtube(app: &AppHandle, opts: &YtDownloadOpts) -> Result<(), String> {
    let url = opts.url.as_str();
    let state = app.state::<AppState>();
    let emit = |v: serde_json::Value| {
        let _ = app.emit("yt", v);
    };

    let needs_setup = ytdlp_path().is_none() || !bin_dir().join("deno.exe").exists();
    if needs_setup {
        emit(serde_json::json!({ "stage": "setup", "status": "Setting up the downloader…" }));
    }
    let yt = ensure_download_tools(&state)?;

    let bin = bin_dir();
    let out_dir = dirs::download_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(std::env::temp_dir);
    std::fs::create_dir_all(&out_dir).ok();
    let pathfile = temp_path("meowverter_ytpath", "txt");
    let _ = std::fs::remove_file(&pathfile);

    let mut args: Vec<String> = vec![
        url.to_string(),
        "--no-playlist".into(),
        "--newline".into(),
        "--no-warnings".into(),
        "--no-simulate".into(),
        "--retries".into(), // ride out transient network/CDN hiccups
        "3".into(),
        "--fragment-retries".into(),
        "3".into(),
        "--concurrent-fragments".into(), // parallel fragment download = much faster
        "4".into(),
        "--ffmpeg-location".into(),
        bin.to_string_lossy().to_string(),
        "--print-to-file".into(),
        "after_move:filepath".into(),
        pathfile.to_string_lossy().to_string(),
        "-o".into(),
        // cap the title at 120 bytes: some sites (Facebook especially) use the
        // whole video description as the "title", which blows past Windows' 260
        // char path limit and the download fails to write
        format!(
            "{}\\%(title).120B [%(id)s].%(ext)s",
            out_dir.to_string_lossy()
        ),
        "--trim-filenames".into(), // extra safety for deep download folders
        "200".into(),
    ];
    if opts.mode == "audio" {
        args.extend(
            ["-x", "--audio-format", "mp3", "--audio-quality", "0"]
                .iter()
                .map(|s| s.to_string()),
        );
    } else {
        let h = match opts.quality.as_str() {
            "" | "best" | "source" => String::new(),
            q => format!("[height<={q}]"),
        };
        let container = if opts.format == "webm" { "webm" } else { "mp4" };
        args.push("-f".into());
        args.push(format!("bv*{h}+ba/b{h}"));
        args.push("--merge-output-format".into());
        args.push(container.to_string());
    }

    // trim a section if both ends are set
    if let (Some(s), Some(e)) = (opts.trim_start, opts.trim_end) {
        if e > s {
            args.push("--download-sections".into());
            args.push(format!("*{s:.2}-{e:.2}"));
            args.push("--force-keyframes-at-cuts".into());
        }
    }

    // put our bin on PATH so yt-dlp finds deno (JS runtime) + ffmpeg
    let path_env = format!(
        "{};{}",
        bin.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );

    let mut child = new_cmd(&yt)
        .args(&args)
        .env("PATH", path_env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to launch yt-dlp: {e}"))?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let err_tail = std::sync::Arc::new(Mutex::new(String::new()));
    let et2 = err_tail.clone();
    let err_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = BufReader::new(stderr).read_to_string(&mut s);
        let tail: String = s
            .chars()
            .rev()
            .take(2000)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        *et2.lock().unwrap() = tail;
    });

    *state.child.lock().unwrap() = Some(child);
    emit(serde_json::json!({ "stage": "progress", "percent": 0.0, "status": "Starting…" }));

    let reader = BufReader::new(stdout);
    let mut last = Instant::now();
    for line in reader.lines() {
        if *state.cancel.lock().unwrap() {
            if let Some(c) = state.child.lock().unwrap().as_mut() {
                let _ = c.kill();
            }
            break;
        }
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.contains("[download]") && line.contains('%') {
            if let Some(pct) = parse_percent(&line) {
                if last.elapsed() > Duration::from_millis(80) {
                    let speed = line
                        .split_whitespace()
                        .find(|t| t.ends_with("/s"))
                        .unwrap_or("");
                    emit(serde_json::json!({
                        "stage": "progress",
                        "percent": pct,
                        "status": if speed.is_empty() { "Downloading…".to_string() } else { format!("Downloading… {speed}") }
                    }));
                    last = Instant::now();
                }
            }
        } else if line.contains("[Merger]")
            || line.contains("Merging")
            || line.contains("[ExtractAudio]")
            || line.contains("[VideoConvertor]")
        {
            emit(
                serde_json::json!({ "stage": "progress", "percent": 99.0, "status": "Finishing up…" }),
            );
        }
    }

    let status = {
        let mut g = state.child.lock().unwrap();
        match g.as_mut() {
            Some(c) => c.wait().map_err(|e| e.to_string())?,
            None => return Ok(()),
        }
    };
    *state.child.lock().unwrap() = None;
    let _ = err_thread.join();

    if *state.cancel.lock().unwrap() {
        emit(serde_json::json!({ "stage": "cancelled" }));
        return Ok(());
    }
    if !status.success() {
        let tail = err_tail.lock().unwrap().clone();
        let msg = tail
            .lines()
            .rev()
            .find(|l| l.to_lowercase().contains("error"))
            .or_else(|| tail.lines().rev().find(|l| !l.trim().is_empty()))
            .unwrap_or("yt-dlp failed")
            .trim()
            .to_string();
        return Err(msg);
    }

    let path = std::fs::read_to_string(&pathfile)
        .unwrap_or_default()
        .trim()
        .to_string();
    if path.is_empty() {
        return Err("Download finished but the saved file couldn't be located.".into());
    }
    emit(serde_json::json!({ "stage": "done", "path": path.clone() }));
    notify(app, "Download done", &leaf(&path));
    Ok(())
}

// ---------------------------------------------------------------------------
// command: in-app self-update (Tauri updater plugin)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct AppUpdateInfo {
    available: bool,
    version: String, // the newer version, if any
    notes: String,
    current: String,
}

/// Ask the update endpoint whether a newer signed release exists.
#[tauri::command]
async fn check_app_update(app: AppHandle) -> Result<AppUpdateInfo, String> {
    use tauri_plugin_updater::UpdaterExt;
    let current = app.package_info().version.to_string();
    let updater = app.updater().map_err(|e| e.to_string())?;
    match updater.check().await {
        Ok(Some(update)) => Ok(AppUpdateInfo {
            available: true,
            version: update.version.clone(),
            notes: update.body.clone().unwrap_or_default(),
            current,
        }),
        Ok(None) => Ok(AppUpdateInfo {
            available: false,
            version: current.clone(),
            notes: String::new(),
            current,
        }),
        Err(e) => Err(e.to_string()),
    }
}

/// Download + verify + install the update, streaming progress, then relaunch.
#[tauri::command]
async fn install_app_update(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("No update is available.")?;

    let mut downloaded: u64 = 0;
    let app2 = app.clone();
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let pct = total
                    .map(|t| (downloaded as f64 / t as f64) * 100.0)
                    .unwrap_or(0.0);
                let _ = app2.emit(
                    "app-update",
                    serde_json::json!({ "stage": "progress", "percent": pct }),
                );
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;

    // new version is installed - relaunch into it (this call never returns)
    app.restart()
}

// ---------------------------------------------------------------------------

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            check_ffmpeg,
            check_ffmpeg_update,
            download_ffmpeg,
            probe,
            thumbnail,
            estimate_gif,
            vmaf,
            start_convert,
            cancel_convert,
            pick_inputs,
            pick_folder,
            pick_output,
            notify_done,
            reveal,
            resolve_music,
            youtube_info,
            youtube_size,
            download_youtube,
            check_app_update,
            install_app_update,
        ])
        .setup(|app| {
            // keep yt-dlp fresh (YouTube regularly breaks older versions) -
            // silent self-update in the background, at most every 3 days
            let tool_handle = app.handle().clone();
            std::thread::spawn(move || {
                let state = tool_handle.state::<AppState>();
                let _setup = state.tool_setup.lock().unwrap();
                let yt = bin_dir().join("yt-dlp.exe");
                if !yt.exists() {
                    return;
                }
                let marker = bin_dir().join("ytdlp_check.txt");
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let last: u64 = std::fs::read_to_string(&marker)
                    .ok()
                    .and_then(|s| s.trim().parse().ok())
                    .unwrap_or(0);
                if now.saturating_sub(last) < 3 * 24 * 3600 {
                    return;
                }
                let updated = new_cmd(&yt)
                    .arg("-U")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
                if updated {
                    let _ = std::fs::write(&marker, now.to_string());
                }
            });

            // forward dropped files to the UI as a "dropped" event
            let handle = app.handle().clone();
            if let Some(win) = app.get_webview_window("main") {
                win.on_window_event(move |ev| {
                    if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop {
                        paths, ..
                    }) = ev
                    {
                        let list: Vec<String> = paths
                            .iter()
                            .map(|p| p.to_string_lossy().to_string())
                            .collect();
                        if !list.is_empty() {
                            let _ = handle.emit("dropped", list);
                        }
                    }
                });
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Meowverter");
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mirrors exactly what the UI sends (camelCase keys).
    fn from_js(json: &str) -> ConvertOpts {
        serde_json::from_str(json).expect("deserialize ConvertOpts")
    }

    #[test]
    fn camelcase_fields_actually_land() {
        let o = from_js(
            r#"{"input":"a.mp4","output":"b.mp4","mode":"video","resolution":"720",
                "format":"mp4_h264","quality":"balanced","targetSizeMb":25.0,
                "trimStart":2.0,"trimEnd":5.0,"fps":15.0,"audioFormat":"mp3",
                "totalDuration":3.5}"#,
        );
        // these are the fields that were silently defaulting before the rename_all fix
        assert_eq!(
            o.total_duration, 3.5,
            "totalDuration must reach total_duration"
        );
        assert_eq!(o.target_size_mb, Some(25.0));
        assert_eq!(o.trim_start, Some(2.0));
        assert_eq!(o.trim_end, Some(5.0));
        assert_eq!(o.audio_format, "mp3");
    }

    #[test]
    fn target_size_builds_two_passes() {
        let o = from_js(
            r#"{"input":"a.mp4","output":"b.mp4","mode":"video","resolution":"source",
                "format":"mp4_h264","quality":"balanced","targetSizeMb":10.0,"totalDuration":60.0}"#,
        );
        assert_eq!(
            build_passes(&o, cpu_encoder("mp4_h264"), false)
                .unwrap()
                .len(),
            2,
            "compress-to-size = two passes"
        );
    }

    #[test]
    fn quality_builds_single_pass() {
        let o = from_js(
            r#"{"input":"a.mp4","output":"b.mp4","mode":"video","resolution":"1080",
                "format":"mp4_h264","quality":"high","totalDuration":60.0}"#,
        );
        assert_eq!(
            build_passes(&o, cpu_encoder("mp4_h264"), false)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn trim_emits_ss_and_t() {
        let o = from_js(
            r#"{"input":"a.mp4","output":"b.mp4","mode":"video","resolution":"source",
                "format":"mp4_h264","quality":"balanced","trimStart":2.0,"trimEnd":5.0,"totalDuration":3.0}"#,
        );
        let a = input_args(&o, false);
        assert!(a.iter().any(|x| x == "-ss"), "trim start -> -ss");
        let t_idx = a.iter().position(|x| x == "-t").expect("trim -> -t");
        assert_eq!(a[t_idx + 1], "3.000", "duration = end - start");
    }

    #[test]
    fn delete_original_recycles_and_keeps_marked_name() {
        let dir = std::env::temp_dir().join("meowverter_delorig_test");
        let _ = std::fs::create_dir_all(&dir);
        let input = dir.join("orig.mkv");
        let output = dir.join("orig_Meowverter.mp4");
        std::fs::write(&input, b"original").unwrap();
        std::fs::write(&output, b"converted").unwrap();
        let o = from_js(&format!(
            r#"{{"input":{:?},"output":{:?},"mode":"video","deleteOriginal":true}}"#,
            input.to_string_lossy(),
            output.to_string_lossy()
        ));
        let final_out = finish_delete_original(&o, 9);
        assert!(!input.exists(), "original should be recycled");
        assert_eq!(
            final_out,
            output.to_string_lossy(),
            "output keeps its _Meowverter name"
        );
        assert!(
            output.exists(),
            "converted file should still be there, marked"
        );
        let _ = std::fs::remove_file(&output);
    }

    #[test]
    fn nvenc_gpu_vs_cpu_decode_args() {
        let o = from_js(
            r#"{"input":"a.mp4","output":"b.mp4","mode":"video","resolution":"1080",
                "format":"mp4_h265","quality":"balanced","totalDuration":60.0}"#,
        );
        let nv = VideoEnc {
            vendor: Vendor::Nvenc,
            name: "hevc_nvenc",
            family: Family::Hevc,
        };
        // full-GPU pipeline: hwaccel + scale_cuda + nvenc, frames stay on GPU (no pix_fmt)
        let g = &build_passes(&o, nv, true).unwrap()[0];
        assert!(g.iter().any(|x| x == "-hwaccel"));
        assert!(g.iter().any(|x| x.contains("scale_cuda")));
        assert!(g.iter().any(|x| x == "hevc_nvenc"));
        assert!(
            !g.iter().any(|x| x == "yuv420p"),
            "gpu path must not force pix_fmt"
        );
        // CPU-decode fallback: no hwaccel, CPU scale, pix_fmt yuv420p
        let c = &build_passes(&o, nv, false).unwrap()[0];
        assert!(!c.iter().any(|x| x == "-hwaccel"));
        assert!(c.iter().any(|x| x.as_str() == "scale=-2:1080"));
        assert!(c.iter().any(|x| x == "hevc_nvenc"));
        assert!(c.iter().any(|x| x == "yuv420p"));
    }

    #[test]
    fn hardware_encoder_args_per_vendor() {
        let o = from_js(
            r#"{"input":"a.mp4","output":"b.mp4","mode":"video","resolution":"source",
                "format":"mp4_h265","quality":"balanced","totalDuration":60.0}"#,
        );
        // Intel QSV -> global_quality, keeps the hevc tag
        let qsv = &build_passes(
            &o,
            VideoEnc {
                vendor: Vendor::Qsv,
                name: "hevc_qsv",
                family: Family::Hevc,
            },
            false,
        )
        .unwrap()[0];
        assert!(qsv.iter().any(|x| x == "hevc_qsv"));
        assert!(qsv.iter().any(|x| x == "-global_quality"));
        assert!(qsv.iter().any(|x| x == "hvc1"));
        // AMD AMF -> constant QP
        let amf = &build_passes(
            &o,
            VideoEnc {
                vendor: Vendor::Amf,
                name: "hevc_amf",
                family: Family::Hevc,
            },
            false,
        )
        .unwrap()[0];
        assert!(amf.iter().any(|x| x == "hevc_amf"));
        assert!(amf.iter().any(|x| x == "-qp_p"));
        // AV1 in mp4: nvenc -> av1_nvenc + cq, no hevc tag
        let oav = from_js(
            r#"{"input":"a.mp4","output":"b.mp4","mode":"video","resolution":"source",
                "format":"mp4_av1","quality":"balanced","totalDuration":60.0}"#,
        );
        let nav = &build_passes(
            &oav,
            VideoEnc {
                vendor: Vendor::Nvenc,
                name: "av1_nvenc",
                family: Family::Av1,
            },
            false,
        )
        .unwrap()[0];
        assert!(nav.iter().any(|x| x == "av1_nvenc"));
        assert!(nav.iter().any(|x| x == "-cq"));
        assert!(!nav.iter().any(|x| x == "hvc1"), "av1 has no hevc tag");
        // AV1 CPU fallback -> libsvtav1 + crf
        let cav = &build_passes(&oav, cpu_encoder("mp4_av1"), false).unwrap()[0];
        assert!(cav.iter().any(|x| x == "libsvtav1"));
        assert!(cav.iter().any(|x| x == "-crf"));
    }

    #[test]
    fn existing_output_gets_a_unique_name() {
        let dir = temp_path("meowverter_collision_test", "");
        std::fs::create_dir_all(&dir).unwrap();
        let existing = dir.join("clip_Meowverter.mp4");
        std::fs::write(&existing, b"keep me").unwrap();

        let picked = available_output_path(existing.to_str().unwrap());
        assert!(picked.ends_with("clip_Meowverter_2.mp4"));
        assert_eq!(std::fs::read(&existing).unwrap(), b"keep me");

        let _ = std::fs::remove_file(&existing);
        let _ = std::fs::remove_dir(&dir);
    }
}
