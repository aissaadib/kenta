# Third-party notices

Kenta is MIT-licensed. It also distributes or depends on the components below.
This file is installed next to `Kenta.exe` and is included in the source
repository.

## FFmpeg and FFprobe

Kenta ships two unmodified FFmpeg executables next to the application:

| File | Purpose |
| --- | --- |
| `ffmpeg.exe` | Capture, concatenation, editing and encoding |
| `ffprobe.exe` | Media inspection |

**Build:** `ffmpeg-n9.0.2-17-g2a571b6068-win64-gpl-9.0` (BtbN Windows builds,
2026-09-30), version string `n9.0.2-17-g2a571b6068`.

**License:** GNU General Public License, version 3. The build configuration
contains `--enable-gpl --enable-version3` and enables GPL components such as
`libx264`. The full license text is installed as `LICENSES/GPL-3.0.txt` and is
also available at <https://www.gnu.org/licenses/gpl-3.0.txt>.

**Corresponding source:**

- Upstream FFmpeg at the exact revision:
  <https://github.com/FFmpeg/FFmpeg/commit/2a571b6068>
- BtbN's build scripts and release artifacts:
  <https://github.com/BtbN/FFmpeg-Builds>
- Source archive for the release series:
  <https://ffmpeg.org/releases/>

The FFmpeg binaries are unmodified. For a physical copy of the corresponding
source, open an issue at <https://github.com/aissaadib/kenta/issues>; source is
available for at least three years from the date this release was distributed,
at no more than the cost of media and shipping.

FFmpeg is a trademark of Fabrice Bellard, originator of the FFmpeg project.
Kenta is not affiliated with or endorsed by the FFmpeg project.

## Microsoft Edge WebView2

Kenta uses the Microsoft Edge WebView2 runtime that ships with Windows 11 and
current Windows 10 installations. If it is missing, the installer can download
the Evergreen Bootstrapper from Microsoft. WebView2 is governed by Microsoft's
terms: <https://developer.microsoft.com/microsoft-edge/webview2/>.

## SQLite

The SQLite library is compiled into Kenta through `rusqlite` with the `bundled`
feature. SQLite is in the public domain:
<https://www.sqlite.org/copyright.html>.

## Rust crates

Kenta's Rust dependencies are listed in `src-tauri/Cargo.toml` and
`src-tauri/Cargo.lock`. They are predominantly MIT, Apache-2.0, or
BSD-licensed. Run `cargo license` or inspect `Cargo.lock` for the exact
dependency set of a build.

## JavaScript packages

The frontend dependencies are listed in `web/package.json` and
`web/package-lock.json`. They are predominantly MIT-licensed. Run
`npx license-checker` in `web/` for a complete report.

## Icons and fonts

Kenta's clapperboard icon is part of the Kenta project. UI icons come from
Lucide (<https://lucide.dev>, ISC license). No third-party fonts are bundled;
the interface uses the system UI font.
