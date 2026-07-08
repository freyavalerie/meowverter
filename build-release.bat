@echo off
rem Build the optimized, console-free release exe.
cd /d "%~dp0src-tauri"
cargo build --release
echo.
echo Done. Exe: src-tauri\target\release\meowverter.exe
pause
