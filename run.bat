@echo off
rem Launch Meowverter. Prefers the optimized release build (no console window);
rem falls back to the debug build; builds one if neither exists yet.
cd /d "%~dp0src-tauri"
if not exist "target\release\meowverter.exe" if not exist "target\debug\meowverter.exe" (
  echo First run - building Meowverter, hang tight...
  cargo build
)
if exist "target\release\meowverter.exe" (
  start "" "target\release\meowverter.exe"
) else (
  start "" "target\debug\meowverter.exe"
)
