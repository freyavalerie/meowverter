// ============================================================
//  Meowverter UI logic
//  Works against the Tauri backend, or a mock layer in a plain
//  browser so the design can be previewed without compiling.
// ============================================================

const TAURI = window.__TAURI__;
const IS_TAURI = !!TAURI;

// ---- mock layer (browser preview) --------------------------
const mockBus = {};
let mockFfUpdated = false;
function mockListen(event, cb) {
  (mockBus[event] = mockBus[event] || []).push(cb);
  return Promise.resolve(() => {});
}
function mockEmit(event, payload) {
  (mockBus[event] || []).forEach((cb) => cb({ payload }));
}
function mockThumb(t) {
  const label = `${Math.floor(t / 60)}:${String(Math.floor(t % 60)).padStart(2, "0")}`;
  const svg = `<svg xmlns='http://www.w3.org/2000/svg' width='480' height='270'><defs><linearGradient id='g' x1='0' y1='0' x2='1' y2='1'><stop offset='0' stop-color='#7c5cff'/><stop offset='1' stop-color='#00e5ff'/></linearGradient></defs><rect width='480' height='270' fill='#0a0a0c'/><rect width='480' height='270' fill='url(#g)' opacity='0.3'/><text x='240' y='150' fill='white' font-family='sans-serif' font-size='34' text-anchor='middle'>${label}</text></svg>`;
  return "data:image/svg+xml;base64," + btoa(svg);
}
async function mockInvoke(cmd, args) {
  await new Promise((r) => setTimeout(r, 120));
  switch (cmd) {
    case "check_ffmpeg":
      return { present: true, on_path: false, ffmpeg: "(mock)" };
    case "check_ffmpeg_update":
      return { available: !mockFfUpdated, current: "20260101", latest: "20260619" };
    case "download_ffmpeg": {
      let p = 0;
      const t = setInterval(() => {
        p += 20;
        if (p >= 100) {
          clearInterval(t);
          mockEmit("setup", { stage: "extract", percent: 100, message: "Unpacking…" });
          setTimeout(() => { mockFfUpdated = true; mockEmit("setup", { stage: "done", percent: 100, message: "ready" }); }, 400);
        } else {
          mockEmit("setup", { stage: "download", percent: p, message: "Downloading…" });
        }
      }, 200);
      return null;
    }
    case "probe":
      return {
        duration: 213.4, width: 1920, height: 1080, vcodec: "h264",
        acodec: "aac", fps: 30, has_audio: true, has_video: true,
        size_bytes: 168_000_000,
      };
    case "pick_inputs":
      return ["C:\\Users\\freya\\Videos\\my cool clip.mp4"];
    case "pick_folder":
      return "D:\\Converted";
    case "notify_done":
      return null;
    case "pick_output":
      return "C:\\Users\\freya\\Videos\\" + args.defaultName;
    case "thumbnail":
      return mockThumb(args.time || 0);
    case "estimate_gif": {
      await new Promise((r) => setTimeout(r, 250));
      const o = args.opts || {};
      const h = o.resolution === "source" ? 360 : Math.min(+o.resolution || 360, 480);
      const w = (h * 16) / 9;
      const qf = { 1: 0.4, 2: 0.6, 3: 0.8, 4: 1.0, 5: 1.35 }[o.gifQuality || 3] || 0.8;
      return Math.round(w * h * (o.fps || 15) * (o.duration || 1) * 0.18 * qf);
    }
    case "youtube_info":
      await new Promise((r) => setTimeout(r, 500));
      return {
        title: "Cool Demo Video - 4K HDR Test Footage", duration: 213.4, thumbnail: mockThumb(0), width: 1920, height: 1080,
        sizes: { best: 168_000_000, "1080": 43_000_000, "720": 19_000_000, "480": 9_500_000, "360": 5_200_000 },
      };
    case "vmaf":
      await new Promise((r) => setTimeout(r, 700));
      return 92.4;
    case "check_app_update":
      return { available: true, version: "0.2.0", notes: "• Instant file drops\n• Keeps the _Meowverter name", current: "0.1.0" };
    case "install_app_update":
      for (let p = 0; p <= 100; p += 25) { mockEmit("app-update", { stage: "progress", percent: p }); await new Promise((r) => setTimeout(r, 120)); }
      return;
    case "youtube_size": {
      await new Promise((r) => setTimeout(r, 300));
      const q = (args.opts || {}).quality;
      const map = { best: 168_000_000, "2160": 168_000_000, "1440": 92_000_000, "1080": 43_000_000, "720": 19_000_000, "480": 9_500_000, "360": 5_200_000 };
      return map[q] || 43_000_000;
    }
    case "download_youtube": {
      let p = 0;
      const t = setInterval(() => {
        p += Math.random() * 12;
        if (p >= 100) {
          clearInterval(t);
          mockEmit("yt", { stage: "progress", percent: 100, status: "Finishing up…" });
          setTimeout(() => mockEmit("yt", { stage: "done", path: "C:\\Users\\freya\\Downloads\\Cool Video [abc123].mp4" }), 450);
        } else {
          mockEmit("yt", { stage: "progress", percent: p, status: "Downloading… 3.2MiB/s" });
        }
      }, 250);
      return null;
    }
    case "start_convert": {
      let p = 0;
      const t = setInterval(() => {
        p += Math.random() * 9;
        if (p >= 100) {
          clearInterval(t);
          mockEmit("convert", { stage: "progress", percent: 100, pass: 1, passes: 1, speed: "3.2x", fps: "240" });
          setTimeout(() => mockEmit("convert", { stage: "done", output: args.opts.output, outputSize: 23_000_000 }), 350);
        } else {
          mockEmit("convert", { stage: "progress", percent: p, pass: 1, passes: 1, speed: "3.2x", fps: "" + Math.round(120 + Math.random() * 200) });
        }
      }, 280);
      return null;
    }
    default:
      return null;
  }
}

const invoke = IS_TAURI ? TAURI.core.invoke : mockInvoke;
const listen = IS_TAURI ? TAURI.event.listen : mockListen;
const winMod = IS_TAURI ? TAURI.window : null;
const appWin = winMod ? winMod.getCurrentWindow() : null;

// ---- keep the window at least as tall as the content ----------
// Coalesces the many fitWindow() calls a single load triggers into ONE
// resize on the next frame, and skips the resize entirely when the height
// hasn't changed - WebView2 briefly blanks the page on every real resize, so
// fewer/smaller resizes = no "blank flash" when files show up.
const MINW = 760;
let fitTimer, fitRaf = 0, fitWantResize = false, lastFitH = 0;
function fitWindow(resize) {
  if (!appWin || !winMod) return;
  if (resize) fitWantResize = true;
  if (fitRaf) return;
  fitRaf = requestAnimationFrame(async () => {
    fitRaf = 0;
    const doResize = fitWantResize; fitWantResize = false;
    const main = document.querySelector("main");
    if (!main) return;
    const padB = parseFloat(getComputedStyle($("app")).paddingBottom) || 26;
    const needed = Math.ceil(main.offsetTop + main.scrollHeight + padB);
    if (needed === lastFitH) return; // height unchanged → no resize, no flash
    try {
      await appWin.setMinSize(new winMod.LogicalSize(MINW, needed));
      if (doResize) {
        const curW = Math.max(MINW, Math.round(window.innerWidth));
        await appWin.setSize(new winMod.LogicalSize(curW, needed));
      }
      lastFitH = needed;
    } catch (e) {
      console.warn("fitWindow failed", e);
    }
  });
}
// user-driven resizes: just keep the min in sync with current wrapping
window.addEventListener("resize", () => {
  clearTimeout(fitTimer);
  fitTimer = setTimeout(() => fitWindow(false), 180);
});

// ---- helpers -----------------------------------------------
const $ = (id) => document.getElementById(id);

function fmtTime(s) {
  s = Math.max(0, Math.round(s));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  if (h) return `${h}:${String(m).padStart(2, "0")}:${String(sec).padStart(2, "0")}`;
  return `${m}:${String(sec).padStart(2, "0")}`;
}
function fmtTimeFine(s) {
  s = Math.max(0, s);
  const m = Math.floor(s / 60);
  const sec = (s % 60).toFixed(2);
  return `${m}:${sec.padStart(5, "0")}`;
}
function fmtBytes(b) {
  if (!b) return "-";
  const u = ["B", "KB", "MB", "GB"];
  let i = 0;
  while (b >= 1024 && i < u.length - 1) { b /= 1024; i++; }
  return `${b.toFixed(b < 10 && i > 0 ? 1 : 0)} ${u[i]}`;
}
function splitPath(p) {
  const sep = p.includes("\\") ? "\\" : "/";
  const i = p.lastIndexOf(sep);
  const dir = i >= 0 ? p.slice(0, i) : "";
  const name = i >= 0 ? p.slice(i + 1) : p;
  const dot = name.lastIndexOf(".");
  const base = dot > 0 ? name.slice(0, dot) : name;
  return { dir, name, base, sep };
}

// ---- state -------------------------------------------------
const state = {
  appMode: "convert",  // "convert" | "youtube"
  input: null,
  info: null,
  ytInfo: null,
  batch: false,
  queue: [],
  ytInfoFailed: false,
  deleteOriginal: localStorage.getItem("meowverter_delorig") === "1",
  batchOutDir: localStorage.getItem("meowverter_batchoutdir") || "", // "" = same folder as each file
  mode: "video",
  resolution: "source",
  format: "mp4_h265",
  quality: "balanced",
  sizeMode: "quality",
  targetMb: 25,
  audioFormat: "mp3",
  fps: 15,
  gifQuality: 3,
  trimStart: 0,   // seconds
  trimEnd: 0,     // seconds
  trimEnabled: false,
  output: null,
  userOutput: false,
};

// ---- ffmpeg availability + auto-update ---------------------
let ffUpdating = false;
async function checkFfmpeg() {
  try {
    const st = await invoke("check_ffmpeg");
    const badge = $("ffBadge");
    if (st.present) {
      badge.classList.add("ok");
      badge.classList.remove("warn", "update", "updating");
      $("ffText").textContent = "ffmpeg ready";
      badge.title = "";
      checkFfmpegUpdate();
    } else {
      badge.classList.add("warn");
      badge.classList.remove("ok", "update", "updating");
      $("ffText").textContent = "ffmpeg needed";
      showSetup();
    }
  } catch (e) {
    $("ffText").textContent = "ffmpeg ?";
  }
}
async function checkFfmpegUpdate() {
  if (ffUpdating) return;
  try {
    const u = await invoke("check_ffmpeg_update");
    if (u.available && !ffUpdating) {
      const badge = $("ffBadge");
      badge.classList.remove("ok");
      badge.classList.add("update");
      $("ffText").textContent = "Update ffmpeg";
      badge.title = `Newer FFmpeg build available (${(u.latest || "").slice(0, 10)}) - click to update`;
    }
  } catch (e) {
    /* offline or rate-limited - just leave it as "ready" */
  }
}
$("ffBadge").addEventListener("click", () => {
  const badge = $("ffBadge");
  if (!badge.classList.contains("update")) return;
  ffUpdating = true;
  badge.classList.remove("update");
  badge.classList.add("updating");
  badge.title = "";
  $("ffText").textContent = "Updating… 0%";
  invoke("download_ffmpeg");
});

// ---- app self-update ---------------------------------------
let appUpdating = false;
async function checkAppUpdate() {
  if (appUpdating) return;
  try {
    const u = await invoke("check_app_update");
    if (u && u.available) {
      const pill = $("appUpdatePill");
      $("appUpdateText").textContent = `Update to ${u.version}`;
      pill.title = u.notes ? `What's new in ${u.version}:\n${u.notes}` : `Version ${u.version} is available`;
      pill.classList.remove("hidden");
    }
  } catch (e) {
    /* no release yet, offline, or rate-limited - just no pill */
  }
}
$("appUpdatePill").addEventListener("click", async () => {
  const pill = $("appUpdatePill");
  if (appUpdating) return;
  if (!confirm("Update Meowverter now?\n\nIt'll download, install, and reopen automatically.")) return;
  appUpdating = true;
  pill.classList.add("busy");
  $("appUpdateText").textContent = "Updating… 0%";
  try {
    await invoke("install_app_update"); // on success the app relaunches (never returns)
  } catch (e) {
    appUpdating = false;
    pill.classList.remove("busy");
    $("appUpdateText").textContent = "Update";
    alert("Couldn't install the update:\n" + e + "\n\nYou can also grab the latest installer from the releases page.");
  }
});
listen("app-update", ({ payload }) => {
  if (payload && payload.stage === "progress") {
    $("appUpdateText").textContent = `Updating… ${Math.round(payload.percent || 0)}%`;
  }
});

function showSetup() {
  $("setupOverlay").classList.remove("hidden");
}
$("dlBtn").addEventListener("click", async () => {
  $("dlBtn").disabled = true;
  $("setupBarWrap").classList.remove("hidden");
  $("setupSub").textContent = "Downloading FFmpeg…";
  await invoke("download_ffmpeg");
});
listen("setup", ({ payload }) => {
  // updating in place via the badge (not the first-install overlay)
  if (ffUpdating) {
    if (payload.stage === "error") {
      $("ffText").textContent = "Update failed";
      ffUpdating = false;
      setTimeout(checkFfmpeg, 1800);
    } else if (payload.stage === "done") {
      ffUpdating = false;
      $("ffBadge").classList.remove("updating");
      $("ffText").textContent = "Updated ✨";
      setTimeout(checkFfmpeg, 1200);
    } else if (payload.stage === "extract") {
      $("ffText").textContent = "Unpacking…";
    } else if (typeof payload.percent === "number") {
      $("ffText").textContent = "Updating… " + Math.round(payload.percent) + "%";
    }
    return;
  }
  if (payload.stage === "error") {
    $("setupSub").textContent = "Hmm, that failed: " + payload.message;
    $("dlBtn").disabled = false;
    return;
  }
  if (payload.stage === "done") {
    $("setupFill").style.width = "100%";
    $("setupSub").textContent = "FFmpeg ready ✨";
    setTimeout(() => {
      $("setupOverlay").classList.add("hidden");
      checkFfmpeg();
    }, 700);
    return;
  }
  if (typeof payload.percent === "number") {
    $("setupFill").style.width = payload.percent + "%";
  }
  if (payload.message) $("setupSub").textContent = payload.message;
});

// ---- loading a file ----------------------------------------
async function pickFile() {
  const paths = await invoke("pick_inputs");
  if (!paths || !paths.length) return;
  if (paths.length === 1) loadFile(paths[0]);
  else loadQueue(paths);
}
async function loadFile(path) {
  state.input = path;
  state.userOutput = false;
  state.batch = false;
  state.queue = [];
  state.info = null;
  document.body.dataset.batch = "off";

  // paint the card shell right away so the drop feels instant - ffprobe on a
  // big file (or a sleeping drive) can take a moment, and we don't make the
  // user stare at a blank page while it runs
  setAppModeVisuals("convert");
  const { name } = splitPath(path);
  $("fileName").textContent = name;
  $("fileName").title = path;
  $("metaChips").innerHTML = `<span class="chip chip-loading">reading…</span>`;
  updateControlsVisibility();
  fitWindow(true);

  let info;
  try {
    info = await invoke("probe", { path });
  } catch (e) {
    alert("Couldn't read that file:\n" + e);
    state.input = null;
    updateControlsVisibility();
    fitWindow(true);
    return;
  }
  state.info = info;
  state.trimStart = 0;
  state.trimEnd = info.duration;
  state.trimEnabled = false;
  // fresh caches for this file; ~120 cached frame steps across its length
  thumbCache.clear();
  gifEstCache.clear();
  thumbGrid = Math.max(0.5, (info.duration || 10) / 120);

  // default content mode: if no video, jump to audio
  setMode(info.has_video ? "video" : "audio");

  const chips = [];
  if (info.has_video) chips.push(`${info.width}×${info.height}`);
  if (info.fps) chips.push(`${info.fps.toFixed(info.fps % 1 ? 2 : 0)} fps`);
  if (info.duration) chips.push(fmtTime(info.duration));
  if (info.vcodec) chips.push(info.vcodec.toUpperCase());
  if (info.has_audio && info.acodec) chips.push("🔊 " + info.acodec.toUpperCase());
  if (info.size_bytes) chips.push(fmtBytes(info.size_bytes));
  $("metaChips").innerHTML = chips.map((c) => `<span class="chip">${c}</span>`).join("");

  // hide upscale presets above the source resolution
  refreshResPills();

  initTrim();
  // trim starts collapsed for each new file
  $("trimToggle").classList.remove("active");
  $("trimToggle").setAttribute("aria-expanded", "false");
  $("trimPanel").classList.add("hidden");
  // match the preview frame to the real video aspect ratio
  if (info.has_video && info.width && info.height) {
    document.querySelector(".frame-wrap.big").style.setProperty("--ar", `${info.width} / ${info.height}`);
  }
  updateTrimStageVisibility();
  recomputeOutput();
  updateEstimate();
  fitWindow(true);
}

// ---- mode + pill wiring ------------------------------------
function setMode(mode) {
  state.mode = mode;
  document.querySelectorAll("#modes .modebtn").forEach((b) => b.classList.toggle("active", b.dataset.mode === mode));
  const idx = ["video", "audio", "gif"].indexOf(mode);
  $("modeGlow").style.transform = `translateX(${idx * 100}%)`;

  $("panel-video").classList.toggle("hidden", mode !== "video");
  $("panel-audio").classList.toggle("hidden", mode !== "audio");
  $("panel-gif").classList.toggle("hidden", mode !== "gif");
  $("resGroup").classList.toggle("hidden", mode === "audio");
  recomputeOutput();
  updateEstimate();
  updateTrimStageVisibility();
  fitWindow(true);
}

document.querySelectorAll("#modes .modebtn").forEach((b) =>
  b.addEventListener("click", () => setMode(b.dataset.mode))
);

function wirePills(containerId, attr, key, after) {
  $(containerId).querySelectorAll(".pill").forEach((b) =>
    b.addEventListener("click", () => {
      $(containerId).querySelectorAll(".pill").forEach((x) => x.classList.remove("active"));
      b.classList.add("active");
      state[key] = b.dataset[attr];
      if (after) after();
    })
  );
}
wirePills("resPills", "res", "resolution", () => { recomputeOutput(); updateEstimate(); });
wirePills("fmtPills", "fmt", "format", () => { recomputeOutput(); updateEstimate(); });
wirePills("qualityRow", "q", "quality", updateEstimate);
wirePills("audioPills", "af", "audioFormat", () => { recomputeOutput(); updateEstimate(); });

// size mode segmented
document.querySelectorAll("#sizeMode .pill").forEach((b) =>
  b.addEventListener("click", () => {
    document.querySelectorAll("#sizeMode .pill").forEach((x) => x.classList.remove("active"));
    b.classList.add("active");
    state.sizeMode = b.dataset.size;
    const target = state.sizeMode === "target";
    $("qualityRow").classList.toggle("hidden", target);
    $("targetRow").classList.toggle("hidden", !target);
    updateEstimate();
    fitWindow(true);
  })
);

// target size chips + slider
$("targetRow").querySelectorAll(".pill").forEach((b) =>
  b.addEventListener("click", () => {
    $("targetRow").querySelectorAll(".pill").forEach((x) => x.classList.remove("active"));
    b.classList.add("active");
    state.targetMb = +b.dataset.mb;
    $("mbSlider").value = state.targetMb;
    $("mbValue").textContent = state.targetMb;
    updateEstimate();
  })
);
$("mbSlider").addEventListener("input", (e) => {
  state.targetMb = +e.target.value;
  $("mbValue").textContent = state.targetMb;
  $("targetRow").querySelectorAll(".pill").forEach((x) => x.classList.remove("active"));
  updateEstimate();
});

// fps slider
$("fpsSlider").addEventListener("input", (e) => {
  state.fps = +e.target.value;
  $("fpsValue").textContent = state.fps;
  updateEstimate();
});

// gif quality slider
const GIF_Q_LABELS = { 1: "Low", 2: "Medium", 3: "Balanced", 4: "High", 5: "Best" };
$("gifQSlider").addEventListener("input", (e) => {
  state.gifQuality = +e.target.value;
  $("gifQValue").textContent = GIF_Q_LABELS[state.gifQuality] || "Balanced";
  updateEstimate();
});

// ---- trim --------------------------------------------------
// sliders work in whole frames so each arrow press = exactly 1 frame
function initTrim() {
  const frames = curFrames();
  for (const id of ["trimStart", "trimEnd"]) {
    $(id).min = 0;
    $(id).max = frames;
    $(id).step = 1;
  }
  $("trimStart").value = 0;
  $("trimEnd").value = frames;
  updateTrim();
}
function updateTrim(which) {
  const fps = curFps();
  const frames = curFrames();
  let a = Math.round(+$("trimStart").value);
  let b = Math.round(+$("trimEnd").value);
  if (a > b - 1) {
    // keep at least a 1-frame gap; push whichever handle didn't just move
    if (which === "start") a = Math.max(0, b - 1);
    else b = Math.min(frames, a + 1);
    $("trimStart").value = a;
    $("trimEnd").value = b;
  }
  state.trimStart = a / fps;
  state.trimEnd = b / fps;
  const pa = frames ? (a / frames) * 100 : 0;
  const pb = frames ? (b / frames) * 100 : 0;
  $("trackFill").style.left = pa + "%";
  $("trackFill").style.width = (pb - pa) + "%";
  $("trimStartLabel").textContent = fmtTimeFine(state.trimStart);
  $("trimEndLabel").textContent = fmtTimeFine(state.trimEnd);
  const trimmed = a > 0 || b < frames;
  $("trimReadout").textContent = trimmed ? `${fmtTime(state.trimEnd - state.trimStart)} clip` : "";
  if (state.trimEnabled) scheduleThumb(which || "start");
  updateEstimate();
}

let thumbTimer;
const thumbCache = new Map();   // grid-time(sec) -> data URL, cleared per source
let thumbGrid = 1;              // seconds between cached frames (set on load)

// Show whichever point is being adjusted in the single big preview.
function scheduleThumb(which) {
  const w = which === "end" ? "end" : "start";
  const t = w === "end" ? Math.max(0, state.trimEnd - 0.05) : state.trimStart;
  $("thumbTag").textContent = (w === "end" ? "OUT · " : "IN · ") + fmtTimeFine(t);
  // YouTube: no local file to grab frames from - show the video's poster
  if (state.appMode === "youtube") {
    if (state.ytInfo && state.ytInfo.thumbnail) $("thumbView").src = state.ytInfo.thumbnail;
    return;
  }
  if (!state.input || !(state.info && state.info.has_video)) return;
  const key = Math.round(t / thumbGrid) * thumbGrid;
  if (thumbCache.has(key)) {            // already grabbed → show instantly, no ffmpeg
    $("thumbView").src = thumbCache.get(key);
    clearTimeout(thumbTimer);
    return;
  }
  clearTimeout(thumbTimer);
  thumbTimer = setTimeout(() => loadThumb(key), 90);
}
async function loadThumb(key) {
  if (thumbCache.has(key)) { $("thumbView").src = thumbCache.get(key); return; }
  try {
    const d = await invoke("thumbnail", { path: state.input, time: Math.max(0, key) });
    if (d) { thumbCache.set(key, d); $("thumbView").src = d; }
  } catch (e) {
    /* a failed frame grab shouldn't break anything */
  }
}
function curDuration() {
  if (state.appMode === "youtube") return state.ytInfo ? state.ytInfo.duration : 0;
  return state.info ? state.info.duration : 0;
}
function curFps() {
  if (state.appMode === "convert" && state.info && state.info.fps > 0) return state.info.fps;
  return 30; // YouTube (fps unknown; trims by time) or missing fps
}
function curFrames() {
  return Math.max(1, Math.round(curDuration() * curFps()));
}
function curHasPreview() {
  if (state.appMode === "youtube") return !!state.ytInfo;
  return !!(state.info && state.info.has_video);
}
function updateTrimStageVisibility() {
  $("trimStage").style.display = curHasPreview() ? "flex" : "none";
}
$("trimStart").addEventListener("input", () => updateTrim("start"));
$("trimEnd").addEventListener("input", () => updateTrim("end"));
$("trimReset").addEventListener("click", initTrim);

// Click a handle, then ←/→ nudge it 1 frame at a time. Hold to accelerate.
let trimHold = 0;
function onTrimKey(e, id, which) {
  if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
  e.preventDefault();
  trimHold = e.repeat ? trimHold + 1 : 0;
  const step = trimHold < 8 ? 1 : trimHold < 20 ? 5 : trimHold < 45 ? 20 : 60;
  const dir = e.key === "ArrowRight" ? 1 : -1;
  const input = $(id);
  let v = Math.round(+input.value) + dir * step;
  v = Math.max(+input.min, Math.min(+input.max, v));
  input.value = v;
  updateTrim(which);
}
$("trimStart").addEventListener("keydown", (e) => onTrimKey(e, "trimStart", "start"));
$("trimEnd").addEventListener("keydown", (e) => onTrimKey(e, "trimEnd", "end"));
$("trimStart").addEventListener("keyup", () => { trimHold = 0; });
$("trimEnd").addEventListener("keyup", () => { trimHold = 0; });
$("trimToggle").addEventListener("click", () => {
  state.trimEnabled = !state.trimEnabled;
  $("trimToggle").classList.toggle("active", state.trimEnabled);
  $("trimToggle").setAttribute("aria-expanded", String(state.trimEnabled));
  $("trimPanel").classList.toggle("hidden", !state.trimEnabled);
  if (state.trimEnabled) {
    updateTrimStageVisibility();
    scheduleThumb("start");
  }
  updateEstimate();
  fitWindow(true);
});

// ---- output path -------------------------------------------
function currentExt() {
  if (state.mode === "audio") return state.audioFormat;
  if (state.mode === "gif") return "gif";
  return { mp4_h264: "mp4", mp4_h265: "mp4", webm: "webm", mkv: "mkv" }[state.format] || "mp4";
}
let lastOutDir = localStorage.getItem("meowverter_outdir") || "";
function recomputeOutput() {
  if (!state.input || state.userOutput) {
    updateOutLabel();
    return;
  }
  const { dir, base, sep } = splitPath(state.input);
  // delete-original mode always saves next to the original (it replaces it)
  const outDir = state.deleteOriginal ? dir : (lastOutDir || dir);
  state.output = `${outDir}${sep}${base}_Meowverter.${currentExt()}`;
  updateOutLabel();
}
function updateOutLabel() {
  if (!state.output) { $("outName").textContent = "-"; return; }
  const { name } = splitPath(state.output);
  $("outName").textContent = name;
  $("outName").title = state.output;
}
$("changeOut").addEventListener("click", async () => {
  const { base } = splitPath(state.input || "output");
  const ext = currentExt();
  const chosen = await invoke("pick_output", { defaultName: `${base}_Meowverter.${ext}`, ext });
  if (chosen) {
    state.output = chosen; state.userOutput = true; updateOutLabel();
    lastOutDir = splitPath(chosen).dir;          // remember this folder for next time
    localStorage.setItem("meowverter_outdir", lastOutDir);
  }
});

// ---- size estimate -----------------------------------------
// Rough H.265/VP9 video bitrate (kbps) by output height, "balanced" baseline.
function bitrateKbps(h) {
  if (h >= 2160) return 14000;
  if (h >= 1440) return 8000;
  if (h >= 1080) return 4500;
  if (h >= 720) return 2500;
  if (h >= 480) return 1200;
  return 700;
}
function estimateBytes() {
  const info = state.info;
  if (!info) return null;
  const dur = Math.max(0.1, state.trimEnabled ? (state.trimEnd - state.trimStart) : info.duration);
  const KBPS = 125; // kbps -> bytes/sec (×1000 ÷ 8)

  if (state.mode === "audio") {
    const kbps = state.audioFormat === "wav" ? 1411 : 192;
    return kbps * KBPS * dur;
  }
  // gif is handled separately (sample-encoded) in updateEstimate
  // video
  if (state.sizeMode === "target") return state.targetMb * 1024 * 1024;
  const h = state.resolution === "source" ? (info.height || 1080) : +state.resolution;
  const qf = { high: 1.7, balanced: 1.0, small: 0.55 }[state.quality] || 1;
  const codecF = state.format === "webm" ? 1.05 : 1.0;
  const videoKbps = bitrateKbps(h) * qf * codecF;
  const audioKbps = info.has_audio ? 160 : 0;
  return (videoKbps + audioKbps) * KBPS * dur;
}
function updateEstimate() {
  if (state.appMode === "youtube") { updateYtEstimate(); return; }
  const el = $("sizeEstimate");
  if (!el) return;
  // GIF: too content-dependent for a formula - encode a short sample and scale it
  if (state.appMode === "convert" && state.mode === "gif" && state.input) {
    scheduleGifEstimate();
    return;
  }
  const bytes = estimateBytes();
  if (bytes == null) { el.textContent = "-"; return; }
  const isTarget = state.mode === "video" && state.sizeMode === "target";
  el.textContent = (isTarget ? "≤ " : "≈ ") + fmtBytes(bytes);
}

let gifEstTimer;
const gifEstCache = new Map();
function gifEstKey() {
  const dur = state.trimEnabled ? Math.max(0.1, state.trimEnd - state.trimStart) : (state.info ? state.info.duration : 0.1);
  const start = state.trimEnabled ? state.trimStart : 0;
  return { key: `${state.resolution}|${state.fps}|${state.gifQuality}|${Math.round(start)}|${Math.round(dur)}`, start, dur };
}
function scheduleGifEstimate() {
  const el = $("sizeEstimate");
  const { key, start, dur } = gifEstKey();
  if (gifEstCache.has(key)) { el.textContent = "≈ " + fmtBytes(gifEstCache.get(key)); return; }
  el.textContent = "≈ …";
  clearTimeout(gifEstTimer);
  gifEstTimer = setTimeout(async () => {
    try {
      const bytes = await invoke("estimate_gif", {
        opts: { input: state.input, resolution: state.resolution, fps: state.fps, gifQuality: state.gifQuality, start, duration: dur },
      });
      gifEstCache.set(key, bytes);
      // only show if these settings are still current (guard against out-of-order results)
      if (gifEstKey().key === key && state.mode === "gif" && state.appMode === "convert") {
        el.textContent = "≈ " + fmtBytes(bytes);
      }
    } catch (e) {
      if (gifEstKey().key === key) el.textContent = "≈ ?";
    }
  }, 450);
}

// ---- youtube download size estimate ------------------------
let ytSizeTimer;
const ytSizeCache = new Map();
function currentYtFrac() {
  const full = state.ytInfo ? state.ytInfo.duration : 0;
  const dur = state.trimEnabled ? Math.max(0.1, state.trimEnd - state.trimStart) : full;
  return full > 0 ? Math.min(1, dur / full) : 1;
}
function showYtSize(fullBytes) {
  const el = $("ytSizeEst");
  if (!el) return;
  if (!fullBytes || fullBytes <= 0) { el.textContent = ""; return; }
  el.textContent = "≈ " + fmtBytes(fullBytes * currentYtFrac());
}
function updateYtEstimate() {
  const el = $("ytSizeEst");
  if (!el) return;
  if (!state.ytInfo) { el.textContent = ""; return; }
  if (state.mode === "audio") {
    // audio is re-encoded to MP3 192k, so size follows that bitrate
    const dur = state.trimEnabled ? Math.max(0.1, state.trimEnd - state.trimStart) : (state.ytInfo.duration || 0.1);
    el.textContent = "≈ " + fmtBytes(192 * 125 * dur);
    return;
  }
  scheduleYtSize();
}
function scheduleYtSize() {
  const el = $("ytSizeEst");
  const url = $("ytUrl").value.trim();
  const quality = state.resolution === "source" ? "best" : state.resolution;
  const key = `${url}|${quality}`;
  if (ytSizeCache.has(key)) { showYtSize(ytSizeCache.get(key)); return; }
  el.textContent = "estimating…";
  clearTimeout(ytSizeTimer);
  ytSizeTimer = setTimeout(async () => {
    try {
      const bytes = await invoke("youtube_size", { opts: { url, quality } });
      ytSizeCache.set(key, bytes);
      const curKey = `${$("ytUrl").value.trim()}|${state.resolution === "source" ? "best" : state.resolution}`;
      if (state.appMode === "youtube" && state.mode !== "audio" && curKey === key) showYtSize(bytes);
    } catch (e) {
      el.textContent = "";
    }
  }, 400);
}

// ---- convert -----------------------------------------------
const RING_C = 326.7;
function setRing(pct) {
  $("ringFg").style.strokeDashoffset = RING_C * (1 - pct / 100);
  $("pctValue").textContent = Math.round(pct);
}
let convDur = 0; // duration of the current convert, for ETA
let vmafCtx = null; // {reference, distorted, refStart} for the quality check
// batch state (declared early so the convert listener can route to it)
let batchActive = -1, batchResolve = null, batchReject = null, batchRunning = false, lastOutputSize = 0;

$("convertBtn").addEventListener("click", () => {
  if (state.appMode === "youtube") return startYoutube();
  if (state.batch) return runBatch();
  startConvert();
});

async function startConvert() {
  if (!state.input) return;
  const full = state.info ? state.info.duration : 0;
  const dur = state.trimEnabled ? Math.max(0.1, state.trimEnd - state.trimStart) : (full || 0.1);
  convDur = dur;

  const opts = {
    input: state.input,
    output: state.output,
    mode: state.mode,
    resolution: state.resolution,
    format: state.format,
    quality: state.quality,
    targetSizeMb: state.mode === "video" && state.sizeMode === "target" ? state.targetMb : null,
    trimStart: state.trimEnabled && state.trimStart > 0.05 ? state.trimStart : null,
    trimEnd: state.trimEnabled && state.trimEnd < full - 0.05 ? state.trimEnd : null,
    fps: state.mode === "gif" ? state.fps : null,
    gifQuality: state.gifQuality,
    audioFormat: state.audioFormat,
    totalDuration: dur,
    deleteOriginal: state.deleteOriginal,
  };

  setRing(0);
  $("progTitle").textContent = "Converting…";
  $("progSub").textContent = "warming up the encoder";
  $("progressOverlay").classList.remove("hidden");

  try {
    await invoke("start_convert", { opts });
  } catch (e) {
    $("progressOverlay").classList.add("hidden");
    alert("Couldn't start:\n" + e);
  }
}

function startYoutube() {
  const url = $("ytUrl").value.trim();
  if (!url) { $("ytUrl").focus(); return; }
  if (!/^https?:\/\/\S+/i.test(url)) { alert("That doesn't look like a video link."); return; }
  const dur = curDuration();
  const opts = {
    url,
    quality: state.resolution === "source" ? "best" : state.resolution,
    format: state.format === "webm" ? "webm" : "mp4",
    mode: state.mode === "audio" ? "audio" : "video",
    trimStart: state.trimEnabled ? state.trimStart : null,
    trimEnd: state.trimEnabled ? state.trimEnd : null,
  };
  setRing(0);
  $("progTitle").textContent = "Downloading…";
  $("progSub").textContent = "reaching YouTube";
  $("progressOverlay").classList.remove("hidden");
  invoke("download_youtube", { opts }).catch((e) => {
    $("progressOverlay").classList.add("hidden");
    alert("Couldn't start:\n" + e);
  });
}

listen("convert", ({ payload }) => {
  // batch mode: route progress/result to the active row
  if (batchActive >= 0) {
    if (payload.stage === "progress") setRowProgress(batchActive, payload.percent || 0);
    else if (payload.stage === "done") { lastOutputSize = payload.outputSize || 0; if (batchResolve) batchResolve(); }
    else if (payload.stage === "error") { if (batchReject) batchReject(payload.message || "failed"); }
    else if (payload.stage === "cancelled") { if (batchReject) batchReject("cancelled"); }
    return;
  }
  if (payload.stage === "progress") {
    setRing(payload.percent || 0);
    const bits = [];
    const sp = parseFloat(payload.speed); // "3.2x" -> 3.2
    if (sp > 0 && convDur > 0 && payload.percent > 0) {
      bits.push(fmtTime((convDur * (1 - payload.percent / 100)) / sp) + " left");
    }
    if (payload.passes > 1) bits.push(`pass ${payload.pass}/${payload.passes}`);
    if (payload.speed) bits.push(payload.speed);
    $("progSub").textContent = bits.join(" · ") || "encoding";
  } else if (payload.stage === "done") {
    setRing(100);
    state.output = payload.output || state.output; // may have taken the original's name
    setTimeout(() => {
      $("progressOverlay").classList.add("hidden");
      const { name } = splitPath(payload.output || state.output);
      let sub = name + " is ready";
      if (payload.outputSize && state.info && state.info.size_bytes) {
        const inB = state.info.size_bytes, outB = payload.outputSize;
        const pct = Math.round((1 - outB / inB) * 100);
        sub = `${fmtBytes(inB)} → ${fmtBytes(outB)} · ${pct >= 0 ? "saved " + pct + "%" : -pct + "% bigger"}`;
      }
      $("doneSub").textContent = sub;
      $("againBtn").textContent = "Convert another";
      // quality check is offered for single video converts only
      const canVmaf = state.appMode === "convert" && state.mode === "video" && !!state.input;
      vmafCtx = canVmaf ? { reference: state.input, distorted: payload.output || state.output, refStart: state.trimEnabled ? state.trimStart : 0 } : null;
      $("vmafResult").classList.add("hidden");
      $("vmafResult").textContent = "";
      const vb = $("vmafBtn");
      vb.classList.toggle("hidden", !canVmaf);
      vb.disabled = false; vb.textContent = "Check quality";
      $("doneOverlay").classList.remove("hidden");
    }, 300);
  } else if (payload.stage === "cancelled") {
    $("progressOverlay").classList.add("hidden");
  } else if (payload.stage === "error") {
    $("progressOverlay").classList.add("hidden");
    alert("Conversion failed:\n" + payload.message);
  }
});

$("vmafBtn").addEventListener("click", async () => {
  if (!vmafCtx) return;
  const vb = $("vmafBtn"); vb.disabled = true; vb.textContent = "Measuring…";
  const res = $("vmafResult");
  res.className = "vmaf-result"; res.classList.remove("hidden");
  res.textContent = "Measuring quality…";
  try {
    const score = await invoke("vmaf", { opts: { reference: vmafCtx.reference, distorted: vmafCtx.distorted, refStart: vmafCtx.refStart, seconds: 20 } });
    const s = Math.round(score * 10) / 10;
    let tier, label;
    if (s >= 95) { tier = "good"; label = "visually identical"; }
    else if (s >= 90) { tier = "good"; label = "excellent"; }
    else if (s >= 80) { tier = "ok"; label = "good"; }
    else { tier = "low"; label = "noticeable quality loss"; }
    res.className = "vmaf-result " + tier;
    res.innerHTML = `VMAF <span class="vq-score">${s}</span> · ${label}`;
    vb.classList.add("hidden");
  } catch (e) {
    res.className = "vmaf-result"; res.textContent = "Couldn't measure quality.";
    vb.disabled = false; vb.textContent = "Check quality";
  }
});

$("cancelBtn").addEventListener("click", () => invoke("cancel_convert"));
$("revealBtn").addEventListener("click", () => invoke("reveal", { path: state.output }));
$("againBtn").addEventListener("click", () => $("doneOverlay").classList.add("hidden"));

// ---- app mode (Upload converter  vs  YouTube downloader) ---
function setAppModeVisuals(mode) {
  state.appMode = mode;
  document.body.dataset.app = mode;
  document.querySelectorAll("#appModeSwitch .modebtn").forEach((b) => b.classList.toggle("active", b.dataset.app === mode));
  $("appModeGlow").style.transform = `translateX(${mode === "youtube" ? 100 : 0}%)`;
  $("resFirstPill").textContent = mode === "youtube" ? "Best" : "Source";
  updateConvertLabel();
}
function updateConvertLabel() {
  const el = $("convertBtn").querySelector(".convert-label");
  if (state.appMode === "youtube") { el.textContent = "Download"; return; }
  if (batchRunning) { el.textContent = "Cancel"; return; }
  if (state.batch) { el.textContent = `Convert ${state.queue.length} file${state.queue.length === 1 ? "" : "s"}`; return; }
  el.textContent = "Convert";
}
function updateControlsVisibility() {
  let show;
  // fail open: info fetch failing shouldn't block downloading
  if (state.appMode === "youtube") show = !!state.ytInfo || state.ytInfoFailed;
  else show = state.batch || !!state.input;
  $("controls").classList.toggle("hidden", !show);
  // trim needs a known duration - hide it when the video info is missing
  const tg = document.querySelector(".trim-group");
  if (tg) tg.classList.toggle("hidden", state.appMode === "youtube" && !state.ytInfo);
  if (state.appMode === "convert" && !state.batch) {
    $("uploadPrompt").classList.toggle("hidden", !!state.input);
    $("fileCard").classList.toggle("hidden", !state.input);
  }
}
function refreshResPills() {
  // hide resolutions higher than the source/video can provide (keep "Source"/"Best")
  const maxH = state.appMode === "youtube"
    ? (state.ytInfo ? state.ytInfo.height : 0)
    : (state.info && state.info.has_video ? state.info.height : 0);
  let activeHidden = false;
  document.querySelectorAll("#resPills .pill").forEach((b) => {
    const r = b.dataset.res;
    const block = r !== "source" && maxH > 0 && +r > maxH;
    b.style.display = block ? "none" : "";
    if (block && b.classList.contains("active")) activeHidden = true;
  });
  // if the picked resolution is no longer available, fall back to Source/Best
  if (activeHidden) {
    document.querySelectorAll("#resPills .pill").forEach((x) => x.classList.remove("active"));
    $("resFirstPill").classList.add("active");
    state.resolution = "source";
  }
}
function setAppMode(mode) {
  // leaving convert clears any batch queue
  state.batch = false;
  state.queue = [];
  document.body.dataset.batch = "off";
  setAppModeVisuals(mode);
  if (mode === "youtube" && state.mode === "gif") setMode("video");
  refreshResPills();
  // reset trim for the new context + pull the active source's dimensions
  state.trimEnabled = false;
  $("trimToggle").classList.remove("active");
  $("trimToggle").setAttribute("aria-expanded", "false");
  $("trimPanel").classList.add("hidden");
  const src = mode === "youtube" ? state.ytInfo : state.info;
  state.trimStart = 0;
  state.trimEnd = src ? src.duration : 0;
  if (src && src.width && src.height) {
    document.querySelector(".frame-wrap.big").style.setProperty("--ar", `${src.width} / ${src.height}`);
  }
  initTrim();
  updateControlsVisibility();
  updateTrimStageVisibility();
  recomputeOutput();
  updateEstimate();
  fitWindow(true);
}
document.querySelectorAll("#appModeSwitch .modebtn").forEach((b) =>
  b.addEventListener("click", () => setAppMode(b.dataset.app))
);

// ---- batch queue (multiple files, shared settings) ---------
function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
}
async function loadQueue(paths) {
  setAppModeVisuals("convert");
  state.input = null;
  state.info = null;
  state.batch = true;
  document.body.dataset.batch = "on";
  state.queue = paths.map((p) => ({ path: p, name: splitPath(p).name, status: "queued", info: null, progress: 0 }));
  setMode("video");
  renderBatchList();
  updateControlsVisibility();
  updateConvertLabel();
  fitWindow(true);
  // probe all files in parallel - each row fills in as its probe lands
  await Promise.all(state.queue.map(async (f) => {
    try { f.info = await invoke("probe", { path: f.path }); } catch (e) { /* keep null */ }
    renderBatchList();
  }));
}
function batchRowStatusText(f) {
  if (f.status === "active") return Math.round(f.progress || 0) + "%";
  if (f.status === "done") return f.savedPct != null ? `done · saved ${f.savedPct}%` : "done";
  if (f.status === "error") return "failed";
  if (f.info) {
    const bits = [];
    if (f.info.has_video && f.info.width) bits.push(`${f.info.width}×${f.info.height}`);
    if (f.info.size_bytes) bits.push(fmtBytes(f.info.size_bytes));
    return bits.join(" · ") || "queued";
  }
  return "queued";
}
function renderBatchList() {
  $("batchCount").textContent = `${state.queue.length} file${state.queue.length === 1 ? "" : "s"}`;
  $("batchRows").innerHTML = state.queue.map((f, i) => {
    const cls = f.status === "done" ? "done" : f.status === "error" ? "error" : f.status === "active" ? "active" : "";
    const x = batchRunning ? "" : `<button class="br-x" data-rm="${i}" title="Remove">×</button>`;
    return `<div class="batch-row ${cls}" data-i="${i}">
      <span class="br-name" title="${escapeHtml(f.path)}">${escapeHtml(f.name)}</span>
      <span class="br-status">${batchRowStatusText(f)}</span>
      ${x}
      <span class="br-fill" style="width:${f.progress || 0}%"></span>
    </div>`;
  }).join("");
}
function setRowProgress(i, pct) {
  const f = state.queue[i];
  if (!f) return;
  f.progress = pct;
  const row = $("batchRows").querySelector(`.batch-row[data-i="${i}"]`);
  if (row) {
    row.querySelector(".br-fill").style.width = pct + "%";
    row.querySelector(".br-status").textContent = Math.round(pct) + "%";
  }
}
function buildBatchOpts(f) {
  const { base, dir, sep } = splitPath(f.path);
  let outDir = dir, joinSep = sep; // default: same folder as each file
  if (state.deleteOriginal) {
    outDir = dir; // replace in place → next to each original
  } else if (state.batchOutDir) {
    outDir = state.batchOutDir.replace(/[\\/]+$/, ""); // one chosen folder for all
    joinSep = outDir.includes("\\") ? "\\" : "/";
  }
  return {
    input: f.path,
    output: `${outDir}${joinSep}${base}_Meowverter.${currentExt()}`,
    mode: state.mode,
    resolution: state.resolution,
    format: state.format,
    quality: state.quality,
    targetSizeMb: state.mode === "video" && state.sizeMode === "target" ? state.targetMb : null,
    trimStart: null,
    trimEnd: null,
    fps: state.mode === "gif" ? state.fps : null,
    gifQuality: state.gifQuality,
    audioFormat: state.audioFormat,
    totalDuration: Math.max(0.1, f.info ? f.info.duration : 0.1),
    silent: true,
    deleteOriginal: state.deleteOriginal,
  };
}
async function runBatch() {
  if (batchRunning) {            // the button acts as Cancel mid-run
    batchRunning = false;
    invoke("cancel_convert");
    return;
  }
  if (!state.queue.some((f) => f.status !== "done")) return;
  batchRunning = true;
  updateConvertLabel();
  renderBatchList();
  for (let i = 0; i < state.queue.length; i++) {
    if (!batchRunning) break;
    const f = state.queue[i];
    if (f.status === "done") continue;
    batchActive = i;
    f.status = "active"; f.progress = 0;
    renderBatchList();
    try {
      await new Promise((resolve, reject) => {
        batchResolve = resolve; batchReject = reject;
        invoke("start_convert", { opts: buildBatchOpts(f) }).catch(reject);
      });
      f.status = "done"; f.progress = 100;
      f.savedPct = (f.info && f.info.size_bytes && lastOutputSize) ? Math.round((1 - lastOutputSize / f.info.size_bytes) * 100) : null;
    } catch (e) {
      batchResolve = batchReject = null;
      if (e === "cancelled") { f.status = "queued"; batchActive = -1; break; }
      f.status = "error";
    }
    batchActive = -1; batchResolve = batchReject = null;
    renderBatchList();
  }
  batchActive = -1; batchRunning = false;
  renderBatchList();
  updateConvertLabel();
  const done = state.queue.filter((f) => f.status === "done").length;
  if (done > 0) invoke("notify_done", { title: "Meowverter", body: `${done} of ${state.queue.length} files converted` });
}
// append files to the queue (dedupes; safe to call mid-run - the run loop
// re-checks queue length each iteration, so added files get converted too)
async function appendToQueue(paths) {
  const have = new Set(state.queue.map((f) => f.path));
  const added = paths.filter((p) => !have.has(p)).map((p) => ({ path: p, name: splitPath(p).name, status: "queued", info: null, progress: 0 }));
  if (!added.length) return;
  state.queue.push(...added);
  renderBatchList(); updateConvertLabel(); fitWindow(true);
  await Promise.all(added.map(async (f) => {
    try { f.info = await invoke("probe", { path: f.path }); } catch (e) {}
    renderBatchList();
  }));
}
$("batchAdd").addEventListener("click", async () => {
  const paths = await invoke("pick_inputs");
  if (paths && paths.length) appendToQueue(paths);
});
$("batchClear").addEventListener("click", () => {
  if (batchRunning) return;
  state.queue = []; state.batch = false; document.body.dataset.batch = "off";
  updateControlsVisibility(); fitWindow(true);
});
$("batchRows").addEventListener("click", (e) => {
  const rm = e.target.getAttribute && e.target.getAttribute("data-rm");
  if (rm == null || batchRunning) return;
  state.queue.splice(+rm, 1);
  if (!state.queue.length) { state.batch = false; document.body.dataset.batch = "off"; updateControlsVisibility(); }
  else { renderBatchList(); updateConvertLabel(); }
  fitWindow(true);
});

// ---- source: upload + drag & drop --------------------------
$("uploadPrompt").addEventListener("click", pickFile);
$("changeFile").addEventListener("click", pickFile);

// one entry point for incoming files: an active queue (or a running batch)
// absorbs drops instead of being replaced by them
function handleIncomingPaths(arr) {
  if (!arr || !arr.length) return;
  if (state.appMode === "convert" && (state.batch || batchRunning)) { appendToQueue(arr); return; }
  if (arr.length === 1) loadFile(arr[0]);
  else loadQueue(arr);
}
listen("dropped", ({ payload }) => {
  handleIncomingPaths(Array.isArray(payload) ? payload : [payload]);
});
window.addEventListener("dragover", (e) => { e.preventDefault(); document.body.classList.add("drag"); });
window.addEventListener("dragleave", () => document.body.classList.remove("drag"));
window.addEventListener("drop", (e) => {
  e.preventDefault();
  document.body.classList.remove("drag");
  if (!IS_TAURI && e.dataTransfer.files.length) {
    handleIncomingPaths([...e.dataTransfer.files].map((f) => f.name));
  }
});
listen("tauri://drag-enter", () => document.body.classList.add("drag"));
listen("tauri://drag-leave", () => document.body.classList.remove("drag"));
listen("tauri://drag-drop", () => document.body.classList.remove("drag"));

// ---- youtube: fetch info as the link is typed --------------
let ytFetchTimer;
$("ytUrl").addEventListener("input", () => {
  const url = $("ytUrl").value.trim();
  clearTimeout(ytFetchTimer);
  state.ytInfo = null;
  state.ytInfoFailed = false;
  ytSizeCache.clear();
  $("ytInfo").classList.add("hidden");
  $("ytFetching").classList.remove("err");
  updateControlsVisibility();
  updateTrimStageVisibility();
  updateEstimate();
  if (!/^https?:\/\/\S+/i.test(url)) { $("ytFetching").classList.add("hidden"); return; }
  $("ytFetching").textContent = "Reading video…";
  $("ytFetching").classList.remove("hidden");
  ytFetchTimer = setTimeout(() => fetchYtInfo(url), 600);
});
// Enter = fetch right away; tap the error line to retry
$("ytUrl").addEventListener("keydown", (e) => {
  if (e.key !== "Enter") return;
  const url = $("ytUrl").value.trim();
  if (!/^https?:\/\/\S+/i.test(url)) return;
  clearTimeout(ytFetchTimer);
  state.ytInfoFailed = false;
  const f = $("ytFetching");
  f.textContent = "Reading video…"; f.classList.remove("err", "hidden");
  fetchYtInfo(url);
});
$("ytFetching").addEventListener("click", () => {
  const f = $("ytFetching");
  if (!f.classList.contains("err")) return;
  const url = $("ytUrl").value.trim();
  if (!/^https?:\/\/\S+/i.test(url)) return;
  state.ytInfoFailed = false;
  f.textContent = "Reading video…"; f.classList.remove("err");
  fetchYtInfo(url);
});
async function fetchYtInfo(url) {
  try {
    const info = await invoke("youtube_info", { url });
    if ($("ytUrl").value.trim() !== url) return; // user kept typing
    state.ytInfo = info;
    $("ytFetching").classList.add("hidden");
    $("ytThumb").src = info.thumbnail || "";
    $("ytTitle").textContent = info.title || "(untitled)";
    const chips = [];
    if (info.width && info.height) chips.push(`${info.width}×${info.height}`);
    if (info.duration) chips.push(fmtTime(info.duration));
    $("ytChips").innerHTML = chips.map((c) => `<span class="chip">${c}</span>`).join("");
    $("ytInfo").classList.remove("hidden");
    if (info.width && info.height) {
      document.querySelector(".frame-wrap.big").style.setProperty("--ar", `${info.width} / ${info.height}`);
    }
    refreshResPills(); // hide resolutions above what the video offers
    state.trimStart = 0;
    state.trimEnd = info.duration;
    initTrim();
    // per-quality sizes come free with the info fetch - no extra requests
    // (rapid repeat requests are what trip YouTube's bot checks)
    if (info.sizes) {
      for (const [q, b] of Object.entries(info.sizes)) {
        if (b > 0) ytSizeCache.set(`${url}|${q}`, b);
      }
    }
    updateControlsVisibility();
    updateTrimStageVisibility();
    updateEstimate();
    fitWindow(true);
  } catch (e) {
    if ($("ytUrl").value.trim() !== url) return;
    // fail open: show the controls anyway - the download itself usually still works
    state.ytInfoFailed = true;
    refreshResPills(); // no known max height -> show all quality pills
    const f = $("ytFetching");
    f.textContent = friendlyYtError(String(e)) + " - tap here to retry, or just hit Download.";
    f.classList.add("err");
    f.classList.remove("hidden");
    updateControlsVisibility();
    updateTrimStageVisibility();
    updateEstimate();
    fitWindow(true);
  }
}

function friendlyYtError(m) {
  m = m || "";
  if (/sign in to confirm|not a bot/i.test(m)) return "YouTube is doing a bot-check right now (it comes and goes)";
  if (/age.?restricted|confirm your age|inappropriate/i.test(m)) return "This video is age-restricted (needs a signed-in account)";
  if (/private video/i.test(m)) return "This video is private";
  if (/copyright|removed/i.test(m)) return "This video was removed";
  if (/not available in your country|geo/i.test(m)) return "This video is region-blocked";
  if (/unavailable/i.test(m)) return "YouTube says this video is unavailable";
  const first = m.replace(/^ERROR:\s*/i, "").split("\n")[0].slice(0, 140);
  return first ? "Couldn't read that link (" + first + ")" : "Couldn't read that link";
}

// ---- youtube download progress (shared overlays) -----------
listen("yt", ({ payload }) => {
  if (payload.stage === "progress" || payload.stage === "setup") {
    setRing(payload.percent || 0);
    $("progTitle").textContent = "Downloading…";
    $("progSub").textContent = payload.status || "downloading";
    $("progressOverlay").classList.remove("hidden");
  } else if (payload.stage === "done") {
    setRing(100);
    state.output = payload.path;
    setTimeout(() => {
      $("progressOverlay").classList.add("hidden");
      const { name } = splitPath(payload.path);
      $("doneSub").textContent = name + " - saved to Downloads";
      $("againBtn").textContent = "Download another";
      $("doneOverlay").classList.remove("hidden");
    }, 300);
  } else if (payload.stage === "cancelled") {
    $("progressOverlay").classList.add("hidden");
  } else if (payload.stage === "error") {
    $("progressOverlay").classList.add("hidden");
    alert("Download failed:\n" + friendlyYtError(payload.message) + "\n\nBot-checks and hiccups usually pass - try again in a moment.");
  }
});

// ---- queue destination folder ------------------------------
function updateBatchOutLabel() {
  const locked = state.deleteOriginal; // delete-original replaces each file in place
  $("batchOutRow").classList.toggle("locked", locked);
  $("batchOutReset").classList.toggle("hidden", locked || !state.batchOutDir);
  const name = $("batchOutName");
  if (locked) {
    name.textContent = "Next to each file (replacing originals)"; name.title = "";
  } else if (state.batchOutDir) {
    name.textContent = state.batchOutDir; name.title = state.batchOutDir;
  } else {
    name.textContent = "Same folder as each file"; name.title = "";
  }
}
$("batchOutChange").addEventListener("click", async () => {
  const chosen = await invoke("pick_folder");
  if (chosen) {
    state.batchOutDir = chosen;
    localStorage.setItem("meowverter_batchoutdir", chosen);
    updateBatchOutLabel();
  }
});
$("batchOutReset").addEventListener("click", () => {
  state.batchOutDir = "";
  localStorage.setItem("meowverter_batchoutdir", "");
  updateBatchOutLabel();
});

// ---- delete-original toggle --------------------------------
function syncDelOrig() {
  $("delOrigBtn").classList.toggle("on", state.deleteOriginal);
  $("delOrigBtn").setAttribute("aria-pressed", String(state.deleteOriginal));
  recomputeOutput();
  updateBatchOutLabel();
}
$("delOrigBtn").addEventListener("click", () => {
  state.deleteOriginal = !state.deleteOriginal;
  localStorage.setItem("meowverter_delorig", state.deleteOriginal ? "1" : "0");
  syncDelOrig();
});
syncDelOrig();
updateBatchOutLabel();

// ---- pause RGB animations when the window isn't focused ----
function setAnimPaused(p) { document.documentElement.classList.toggle("anim-paused", p); }
document.addEventListener("visibilitychange", () => setAnimPaused(document.hidden));
window.addEventListener("blur", () => setAnimPaused(true));
window.addEventListener("focus", () => setAnimPaused(false));

// ---- boot --------------------------------------------------
setAppModeVisuals("convert");
updateControlsVisibility();
checkFfmpeg();
// re-check for an ffmpeg update every 6 hours while the app stays open
setInterval(checkFfmpegUpdate, 6 * 60 * 60 * 1000);
// check for a new Meowverter version on launch, then every 6 hours
checkAppUpdate();
setInterval(checkAppUpdate, 6 * 60 * 60 * 1000);
