# FFmpeg in Kenta

Kenta does not require a separate FFmpeg installation in the released Windows
build. `ffmpeg.exe` and `ffprobe.exe` are installed next to `Kenta.exe`.

## Resolution order

The app looks for the tools in this order:

1. `KENTA_FFMPEG` / `KENTA_FFPROBE` environment variables (development
   override).
2. `ffmpeg.exe` / `ffprobe.exe` next to `Kenta.exe` (the packaged build).
3. `ffmpeg` / `ffprobe` on `PATH`.

The packaged installer ships the first two files. A developer build can still
use a system FFmpeg.

## Shipped build

- Package: `ffmpeg-n9.0.2-17-g2a571b6068-win64-gpl-9.0`
- Provider: [BtbN FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds)
- Configuration: GPL v3, static, with `--enable-ffnvcodec`,
  `--enable-cuda-llvm`, `--enable-libx264`, `--enable-libx265`,
  `--enable-schannel`, and the `gfxcapture` Windows Graphics Capture filter.
- License: GPL-3.0. See `THIRD_PARTY_NOTICES.md` and `GPL-3.0.txt`.

The GPL build is required for two reasons: it includes `libx264` for the CPU
fallback, and it includes `gfxcapture` for smooth Windows Graphics Capture.
An LGPL-only build would remove the CPU encoder fallback.

## Verifying a local build

```powershell
.\ffmpeg.exe -version
.\ffprobe.exe -version
.\ffmpeg.exe -hide_banner -encoders | Select-String nvenc
.\ffmpeg.exe -hide_banner -filters | Select-String gfxcapture
```

If the binaries are missing, Kenta still starts and reports that FFmpeg is not
available instead of silently producing empty clips.
