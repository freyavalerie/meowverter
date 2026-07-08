# Catverter

A little FFmpeg app that stays out of your way. Pure black, rainbow, and quick.
Convert videos, pull the audio out, make GIFs, or grab a clip off YouTube and a
bunch of other sites. It leans on your GPU (NVENC) to encode, so things move fast
and your CPU stays free.

![Catverter converting a video](docs/screenshot.png)

## Download

Head to the [releases page](https://github.com/freyavalerie/catverter/releases/latest),
grab `Catverter_x.x.x_x64-setup.exe`, and run it. Windows 10 and 11, 64-bit.

The first time you run it, Windows might throw up a SmartScreen warning about an
unknown publisher. That's only because I haven't bought a signing certificate.
Click "More info", then "Run anyway", and it won't ask again. From there the app
keeps itself up to date on its own.

FFmpeg downloads itself the first time you open the app, so there's nothing else
to set up.

## What it does

- Convert to MP4 (H.265) or WebM, either by quality or a target file size (handy
  when Discord won't take your clip)
- Encodes on the GPU, and falls back to the CPU if your card can't do a format
- Rip audio to MP3, M4A, or WAV, or turn a clip into a GIF with a quality slider
- Trim down to the exact frame, with a live preview
- Download from YouTube, TikTok, X, Twitch, and plenty more
- Drop a whole folder in at once, set your options once, and let it work through
  the queue
- Optionally send the original to the Recycle Bin and keep a tidy
  `name_Catverter.mp4`
- One click gives you a quality score so you can see how close the result looks to
  the original
- Updates itself from inside the app

## Building it yourself

No Node, no npm. You just need [Rust](https://rustup.rs) and the Tauri CLI:

```sh
cargo install tauri-cli --version "^2"
cd src-tauri
cargo tauri dev      # run it
cargo tauri build    # build a release plus installer
```

The interface is plain HTML, CSS, and JavaScript in `ui/`. The backend is Rust in
`src-tauri/`, and it spawns a real ffmpeg and streams progress back to the window.
The frontend has a small mock layer, so you can also just open the `ui/` folder in
a browser to poke at the design without compiling anything.

## Cutting a release

Updates are signed with a private key that lives outside this repo (in
`~/.catverter/`). Back it up. If it goes missing, existing installs won't trust new
updates. The public half sits in `src-tauri/tauri.conf.json`.

One command builds it, signs it, writes the update manifest, and pushes a GitHub
release:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\release.ps1 -Version 0.2.0 -Notes "what changed"
```

Anyone who already has it installed gets the update within a few hours, or the next
time they open it.
