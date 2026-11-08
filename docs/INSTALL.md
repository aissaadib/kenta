# Installing Kenta on Windows

## Requirements

- Windows 10 or Windows 11, 64-bit.
- Microsoft Edge WebView2. It is included with Windows 11 and current Windows
  10. The installer can download the Evergreen Bootstrapper if it is missing,
  so the first install may need an internet connection.
- Optional: an NVIDIA GPU for NVENC. Kenta falls back to CPU x264 when NVENC is
  unavailable.
- Optional: a desktop loopback device (Stereo Mix, a virtual cable, or an
  NVIDIA/Realtek loopback) if you want desktop audio.

No Rust, Node.js, Visual Studio, or separate FFmpeg installation is required.

## Install

1. Download `Kenta_0.2.0_x64-setup.exe` from the
   [Releases page](https://github.com/aissaadib/kenta/releases).
2. Run the installer. It installs per-user and does not require administrator
   rights.
3. Launch **Kenta** from the Start Menu.

The installer is currently **unsigned**. Windows SmartScreen may show
"Windows protected your PC". Choose **More info → Run anyway** only if you
downloaded the file from this repository and the SHA-256 matches the release
checksum.

## First run

- Kenta starts buffering the selected monitor automatically.
- The default output folder is your Videos folder; choose another folder in
  **Settings → Clips & storage**.
- Press **F8** (or **Save last …**) to save the replay buffer.
- Open **Clips** to preview, edit, rename, or upload a clip.

## Upgrade

Run a newer installer over the existing installation. The installer replaces
application files only. Your clips, settings, library database, and Google
Drive credential are stored outside the installation directory and are kept.

## Uninstall

Use **Settings → Apps → Installed apps → Kenta → Uninstall**, or the uninstall
shortcut in the Kenta Start Menu folder.

Uninstalling removes the application and its shortcuts. It does **not** delete
your recordings, the clip library database, settings, or files already uploaded
to Google Drive.

## Data locations

| Data | Location |
| --- | --- |
| Settings | `%APPDATA%\Kenta\settings.json` |
| Library database | `%USERPROFILE%\Videos\Kenta\library.db` |
| Clips | The folder chosen in Settings (default: `%USERPROFILE%\Videos`) |
| Thumbnail cache | `%TEMP%\kenta_thumbs` |
| Replay segments | `%TEMP%\kenta_buffer` |
| Google refresh token | Windows Credential Manager, `Kenta/google-drive/<account id>` |

See [PRIVACY.md](PRIVACY.md) for the full data-handling description.
