#Requires -Version 5.1
<#
.SYNOPSIS
  Stage the distributable Kenta release folder from a finished NSIS build.

.DESCRIPTION
  Copies the installer produced by `cargo tauri build --bundles nsis` into a
  clean Kenta-Release folder together with the checksum file and the licences
  that must travel with the binaries. Nothing else is copied: no target
  directories, no debug builds, no source maps, no logs.

  Kenta-Release is gitignored. Installers are published as GitHub release
  assets, never committed.

.EXAMPLE
  pwsh -File scripts/stage-release.ps1
  pwsh -File scripts/stage-release.ps1 -Version 0.2.0 -OutDir D:\Kenta-0.2.0
#>
[CmdletBinding()]
param(
  [string]$Version,
  [string]$OutDir
)

$ErrorActionPreference = "Stop"

$root = Split-Path $PSScriptRoot -Parent
if (-not $OutDir) {
  $OutDir = Join-Path $root "Kenta-Release"
}

$confPath = Join-Path $root "src-tauri/tauri.conf.json"
if (-not $Version) {
  $Version = (Get-Content $confPath -Raw | ConvertFrom-Json).version
}

$installer = Join-Path $root "src-tauri/target/release/bundle/nsis/Kenta_${Version}_x64-setup.exe"
if (-not (Test-Path -LiteralPath $installer)) {
  throw "Installer not found: $installer`nRun 'cargo tauri build --bundles nsis' in src-tauri first."
}

# Only ever clear a folder this script owns, so a mistyped -OutDir cannot wipe
# something else.
if ((Split-Path $OutDir -Leaf) -ne "Kenta-Release") {
  throw "-OutDir must end in a folder named 'Kenta-Release' (got '$OutDir')."
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
Get-ChildItem -LiteralPath $OutDir -File | Remove-Item -Force

$installerName = "Kenta_Installer.exe"
Copy-Item -LiteralPath $installer -Destination (Join-Path $OutDir $installerName) -Force
Copy-Item -LiteralPath (Join-Path $root "LICENSE") -Destination (Join-Path $OutDir "LICENSE.txt") -Force
Copy-Item -LiteralPath (Join-Path $root "THIRD_PARTY_NOTICES.md") -Destination (Join-Path $OutDir "THIRD_PARTY_NOTICES.txt") -Force

$licenses = Join-Path $OutDir "LICENSES"
New-Item -ItemType Directory -Force -Path $licenses | Out-Null
Copy-Item -LiteralPath (Join-Path $root "LICENSES/GPL-3.0.txt") -Destination (Join-Path $licenses "GPL-3.0.txt") -Force

$readme = @"
Kenta $Version - Windows x64

Kenta_Installer.exe
  Per-user Windows installer. No administrator rights are required.
  Microsoft Edge WebView2 is already present on Windows 11 and current
  Windows 10; otherwise the installer downloads the Evergreen Bootstrapper.
  FFmpeg and FFprobe are included, so no separate install is needed.

Verify the download before running it
  SHA256SUMS.txt holds the SHA-256 of Kenta_Installer.exe. Check it with:

    Get-FileHash .\Kenta_Installer.exe -Algorithm SHA256

  The output must match the hash in SHA256SUMS.txt.

The installer is unsigned
  Windows SmartScreen may show "Windows protected your PC". Choose
  More info -> Run anyway only if the hash matches.

Upgrading
  Install a newer build over the existing one. Clips, settings, the clip
  library and the Google Drive credential live outside the install directory
  and are kept.

Uninstalling
  Settings -> Apps -> Installed apps -> Kenta, or the Kenta Start Menu folder.
  Uninstalling removes the application and its shortcuts. Recordings, the
  library database, settings and files already uploaded to Google Drive stay.

Licences
  Kenta is MIT-licensed (LICENSE.txt). The bundled FFmpeg and FFprobe
  binaries are GPL-3.0 (LICENSES/GPL-3.0.txt). See THIRD_PARTY_NOTICES.txt.
"@

# ASCII keeps the file readable in Notepad on any Windows locale.
$readme | Set-Content -LiteralPath (Join-Path $OutDir "README.txt") -Encoding ascii

$hash = (Get-FileHash -LiteralPath (Join-Path $OutDir $installerName) -Algorithm SHA256).Hash.ToLowerInvariant()
# Two spaces, LF endings: that is what `sha256sum -c` expects, and CRLF would
# make the filename unreadable to it.
$sums = Join-Path $OutDir "SHA256SUMS.txt"
[System.IO.File]::WriteAllText($sums, "$hash  $installerName`n", [System.Text.Encoding]::ASCII)

Write-Host "Staged Kenta $Version in $OutDir"
Get-ChildItem -LiteralPath $OutDir -Recurse -File |
  ForEach-Object { "{0,10:N1} MB  {1}" -f ($_.Length / 1MB), $_.FullName.Substring($OutDir.Length + 1) }
Write-Host "SHA-256 $hash"
