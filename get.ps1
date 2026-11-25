# Installs Everwall on Windows 11 from the latest release, for the current
# user and without administrator rights. In PowerShell:
#
#   irm https://raw.githubusercontent.com/johan-socarras/Everwall/main/get.ps1 | iex
#
# It downloads Everwall-<version>-windows-x64.zip from GitHub, checks it
# against the SHA-256 GitHub publishes for it, unpacks it into
# %LOCALAPPDATA%\Programs\Everwall, adds Everwall to the Start menu and starts
# it. Running it again updates Everwall in place; an open Everwall is closed
# first and put back in the tray. Optional:
#
#   $env:EVERWALL_VERSION = 'v1.0.0'   install that release instead of the latest
#   $env:GITHUB_TOKEN = '...'          for a private fork, or to avoid rate limits
#
# Works in Windows PowerShell 5.1 and PowerShell 7. To build from the source
# instead, use install.ps1 in a clone of the repository.

& {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'   # Invoke-WebRequest is much faster without it
    $repo = 'johan-socarras/Everwall'

    function Stop-Install($message) {
        Write-Host $message -ForegroundColor Red
        throw 'Everwall was not installed.'
    }

    if ([Environment]::OSVersion.Platform -ne 'Win32NT') {
        Stop-Install 'This installer is for Windows. On Ubuntu, use get.sh: see the README.'
    }
    if (-not [Environment]::Is64BitOperatingSystem) {
        Stop-Install 'Everwall needs 64-bit Windows.'
    }
    # Windows PowerShell 5.1 may still offer only older TLS versions.
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $headers = @{ 'User-Agent' = 'everwall-install'; 'Accept' = 'application/vnd.github+json' }
    if ($env:GITHUB_TOKEN) { $headers['Authorization'] = "Bearer $env:GITHUB_TOKEN" }
    $which = if ($env:EVERWALL_VERSION) { "tags/$env:EVERWALL_VERSION" } else { 'latest' }
    try {
        $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases/$which" -Headers $headers
    } catch {
        Stop-Install "Could not find the release '$which' of $repo on GitHub: $($_.Exception.Message)"
    }
    $asset = $release.assets | Where-Object { $_.name -match '^Everwall-.+-windows-x64\.zip$' } | Select-Object -First 1
    if (-not $asset) { Stop-Install "The release $($release.tag_name) has no Windows package." }

    $destination = Join-Path $env:LOCALAPPDATA 'Programs\Everwall'
    $folder = Join-Path ([IO.Path]::GetTempPath()) ("everwall-install-" + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory $folder | Out-Null
    try {
        $zip = Join-Path $folder $asset.name
        Write-Host "Downloading $($asset.name) ($([Math]::Round($asset.size / 1MB, 1)) MB)..."
        if ($env:GITHUB_TOKEN) {
            # A private repository only serves its files through the API.
            $download = @{ 'User-Agent' = 'everwall-install'; 'Accept' = 'application/octet-stream'; 'Authorization' = "Bearer $env:GITHUB_TOKEN" }
            Invoke-WebRequest -Uri $asset.url -Headers $download -OutFile $zip -UseBasicParsing
        } else {
            Invoke-WebRequest -Uri $asset.browser_download_url -Headers @{ 'User-Agent' = 'everwall-install' } -OutFile $zip -UseBasicParsing
        }

        $size = (Get-Item $zip).Length
        if ($size -ne $asset.size) { Stop-Install "The download is incomplete: $size bytes of $($asset.size)." }
        if ($asset.digest -match '^sha256:([0-9a-f]{64})$') {
            $hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLowerInvariant()
            if ($hash -ne $Matches[1]) { Stop-Install 'The download does not match the SHA-256 GitHub publishes for it.' }
            Write-Host 'Checked against the SHA-256 published on GitHub.'
        } else {
            Write-Host 'GitHub publishes no SHA-256 for this file; checked its size only.' -ForegroundColor Yellow
        }

        $unpacked = Join-Path $folder 'unpacked'
        Expand-Archive -Path $zip -DestinationPath $unpacked
        foreach ($file in 'Everwall.exe', 'WebView2Loader.dll') {
            if (-not (Test-Path (Join-Path $unpacked $file))) { Stop-Install "The package has no $file." }
        }

        Write-Host "Installing Everwall $($release.tag_name) for $env:USERNAME..."
        # Only now, so a failed download leaves the running copy alone.
        $running = Get-Process Everwall -ErrorAction SilentlyContinue
        if ($running) {
            $running | Stop-Process -Force
            Start-Sleep -Milliseconds 800
        }
        New-Item -ItemType Directory -Force $destination | Out-Null
        Copy-Item (Join-Path $unpacked '*') $destination -Force
    } finally {
        Remove-Item $folder -Recurse -Force -ErrorAction SilentlyContinue
    }

    $exe = Join-Path $destination 'Everwall.exe'
    $shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Everwall.lnk'
    $shell = New-Object -ComObject WScript.Shell
    $lnk = $shell.CreateShortcut($shortcut)
    $lnk.TargetPath = $exe
    $lnk.WorkingDirectory = $destination
    $lnk.Description = 'A different wallpaper on each screen'
    $lnk.Save()

    # The app adds itself to the programs that start with Windows.
    if ($running) {
        Start-Process $exe -ArgumentList '--tray'
    } else {
        Start-Process $exe
    }
    Write-Host "Everwall $($release.tag_name) is installed in $destination." -ForegroundColor Green
    Write-Host 'It is in the tray and in the Start menu.'
}
