<#
  Meowverter release - one command to ship an update.

  What it does:
    1. (optional) bumps the version in tauri.conf.json + Cargo.toml
    2. builds the signed NSIS installer + updater signature
    3. writes latest.json (the manifest the app checks for updates)
    4. publishes a GitHub Release with the installer, .sig, and latest.json

  Usage (from the Meowverter folder):
    powershell -ExecutionPolicy Bypass -File scripts\release.ps1 -Version 0.2.0 -Notes "Faster drops, bug fixes"
    powershell -ExecutionPolicy Bypass -File scripts\release.ps1 -Notes "Small fixes"   # keep current version

  Requirements:
    - gh CLI logged in as the account that owns the repo (freyavalerie)
    - the signing key at  %USERPROFILE%\.meowverter\updater.key
    - a git remote 'origin' pointing at the GitHub repo
#>
param(
  [string]$Version = "",
  [string]$Notes   = "",
  [string]$Repo    = "freyavalerie/meowverter"
)

$ErrorActionPreference = "Stop"
$root    = Split-Path $PSScriptRoot -Parent
$src     = Join-Path $root "src-tauri"
$conf    = Join-Path $src "tauri.conf.json"
$cargo   = Join-Path $src "Cargo.toml"
$keyFile = Join-Path $env:USERPROFILE ".meowverter\updater.key"

function Fail($m) { Write-Host "`n[X] $m" -ForegroundColor Red; exit 1 }

# --- 0. sanity ---
if (-not (Test-Path $keyFile)) { Fail "Signing key not found at $keyFile (back this up - it's the only way to ship trusted updates)." }
if (-not (Get-Command gh -ErrorAction SilentlyContinue)) { Fail "GitHub CLI (gh) not found on PATH." }

# --- 1. optional version bump ---
if ($Version -ne "") {
  Write-Host "Bumping version -> $Version"
  $c = Get-Content $conf -Raw
  $c = [regex]::Replace($c, '("version":\s*")[^"]*(")', "`${1}$Version`${2}", 1)
  Set-Content $conf $c -Encoding utf8 -NoNewline
  $g = Get-Content $cargo -Raw
  $g = [regex]::Replace($g, '(?m)^(version\s*=\s*")[^"]*(")', "`${1}$Version`${2}", 1)
  Set-Content $cargo $g -Encoding utf8 -NoNewline
}

# read the version we're shipping
$verMatch = [regex]::Match((Get-Content $conf -Raw), '"version":\s*"([^"]+)"')
if (-not $verMatch.Success) { Fail "Couldn't read version from tauri.conf.json" }
$ver = $verMatch.Groups[1].Value
$tag = "v$ver"
Write-Host "Releasing Meowverter $tag" -ForegroundColor Cyan

# --- 2. build signed ---
$env:TAURI_SIGNING_PRIVATE_KEY          = (Get-Content $keyFile -Raw)
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
Stop-Process -Name meowverter -Force -ErrorAction SilentlyContinue
Push-Location $src
try {
  cargo tauri build --bundles nsis
  if ($LASTEXITCODE -ne 0) { Fail "cargo tauri build failed" }
} finally { Pop-Location }

$bundle = Join-Path $src "target\release\bundle\nsis"
$exe = Get-ChildItem "$bundle\*_x64-setup.exe"     | Select-Object -First 1
$sig = Get-ChildItem "$bundle\*_x64-setup.exe.sig" | Select-Object -First 1
if (-not $exe -or -not $sig) { Fail "build didn't produce both the installer and its .sig" }

# --- 3. write latest.json (the updater manifest) ---
$manifest = [ordered]@{
  version   = $ver
  notes     = $Notes
  pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
  platforms = [ordered]@{
    "windows-x86_64" = [ordered]@{
      signature = (Get-Content $sig.FullName -Raw).Trim()
      url       = "https://github.com/$Repo/releases/download/$tag/$($exe.Name)"
    }
  }
}
$latest = Join-Path $bundle "latest.json"
$manifest | ConvertTo-Json -Depth 6 | Set-Content $latest -Encoding utf8
Write-Host "Wrote $latest"

# --- 4. publish ---
Write-Host "Publishing GitHub release $tag to $Repo ..."
gh release create $tag $exe.FullName $sig.FullName $latest `
  --repo $Repo --title "Meowverter $ver" --notes $Notes
if ($LASTEXITCODE -ne 0) { Fail "gh release create failed (is gh logged in as the repo owner?)" }

Write-Host "`n[OK] Meowverter $ver published. Existing installs will offer the update within a few hours (or on next launch)." -ForegroundColor Green
