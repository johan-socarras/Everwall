# Builds Everwall and packs the zip attached to each GitHub release:
# Everwall.exe, WebView2Loader.dll, its licence and the licences of the
# libraries built into it (THIRD-PARTY-LICENSES.txt), ready to unzip and run.
#   pwsh -File package.ps1            build and pack
#   pwsh -File package.ps1 -NoBuild   pack the last build
# The zip is written next to the build (CARGO_TARGET_DIR or src-tauri\target),
# never inside the repository.
param([switch]$NoBuild)
$ErrorActionPreference = 'Stop'

$tauri = Join-Path $PSScriptRoot 'src-tauri'
# The exe keeps source paths for its error messages: they must not carry the
# user folder of whoever built it.
$env:CARGO_ENCODED_RUSTFLAGS = "--remap-path-prefix=$env:USERPROFILE=~`u{1F}--remap-path-prefix=$PSScriptRoot=everwall"
if (-not $NoBuild) {
    Push-Location $tauri
    try {
        cargo build --release
        if ($LASTEXITCODE) { throw 'The build failed.' }
    } finally { Pop-Location }
}

$version = (Get-Content (Join-Path $tauri 'tauri.conf.json') -Raw | ConvertFrom-Json).version
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $tauri 'target' }
$source = Join-Path $target 'release'
$stage = Join-Path $target "Everwall-$version"
$zip = Join-Path $target "Everwall-$version-windows-x64.zip"

if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory $stage | Out-Null
Copy-Item (Join-Path $source 'everwall.exe') (Join-Path $stage 'Everwall.exe')
Copy-Item (Join-Path $source 'WebView2Loader.dll') $stage
Copy-Item (Join-Path $PSScriptRoot 'LICENSE') (Join-Path $stage 'LICENSE.txt')
Copy-Item (Join-Path $PSScriptRoot 'THIRD-PARTY-LICENSES.txt') $stage

if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip
Remove-Item $stage -Recurse -Force
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
"$zip"
"SHA-256 $hash"
