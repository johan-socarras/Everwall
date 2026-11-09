# Builds Everwall and installs it for the current user in
# %LOCALAPPDATA%\Programs\Everwall, with a Start menu shortcut. If it was
# running, it is closed, replaced and put back in the tray.
#   pwsh -File install.ps1            build and install
#   pwsh -File install.ps1 -NoBuild   install the last build
param([switch]$NoBuild)
$ErrorActionPreference = 'Stop'

$tauri = Join-Path $PSScriptRoot 'src-tauri'
if (-not $NoBuild) {
    Push-Location $tauri
    try {
        cargo build --release
        if ($LASTEXITCODE) { throw 'The build failed.' }
    } finally { Pop-Location }
}

# cargo puts the .exe in CARGO_TARGET_DIR when it is set
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $tauri 'target' }
$source = Join-Path $target 'release'
$destination = Join-Path $env:LOCALAPPDATA 'Programs\Everwall'
New-Item -ItemType Directory -Force $destination | Out-Null

$running = Get-Process Everwall -ErrorAction SilentlyContinue
if ($running) {
    $running | Stop-Process -Force
    Start-Sleep -Milliseconds 800
}

Copy-Item (Join-Path $source 'everwall.exe') (Join-Path $destination 'Everwall.exe') -Force
# with Rust's GNU toolchain, WebView2 needs its loader next to the exe
Copy-Item (Join-Path $source 'WebView2Loader.dll') $destination -Force

$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Everwall.lnk'
$shell = New-Object -ComObject WScript.Shell
$lnk = $shell.CreateShortcut($shortcut)
$lnk.TargetPath = Join-Path $destination 'Everwall.exe'
$lnk.WorkingDirectory = $destination
$lnk.Description = 'A different wallpaper on each screen'
$lnk.Save()

$exe = Join-Path $destination 'Everwall.exe'
if ($running) {
    Start-Process $exe -ArgumentList '--tray'
} else {
    Start-Process $exe
}
"Installed in $destination"
