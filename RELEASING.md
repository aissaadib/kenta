# Releasing Kenta

Kenta's first public release is v0.1.0. This document describes how to cut
future releases. It is written for the project maintainer.

## Overview

1. Push a `vX.Y.Z` tag to `origin`.
2. GitHub Actions builds the NSIS installer, computes SHA-256 checksums, and
   opens a **draft** GitHub Release.
3. A maintainer tests the installer on a clean Windows machine.
4. The maintainer publishes the release.

Steps 1 and 2 are automated. Steps 3 and 4 require manual review.

## Before you tag

- The branch being released from must be clean (`git status --short` empty).
- All checks must pass:
  - `cargo test --release --lib` (in `src-tauri/`)
  - `npx tsc --noEmit`, `npm run lint`, `npm run build` (in `web/`)
- The version in `src-tauri/tauri.conf.json` and `web/package.json` must match
  the tag you are about to push.
- `google-oauth.json` must exist locally for a development build (it is
  gitignored and never committed). CI creates it from
  `GOOGLE_OAUTH_CLIENT_ID` / `GOOGLE_OAUTH_CLIENT_SECRET` secrets.

## Cutting a release

1. Create and push an annotated tag:

   ```powershell
   git tag -a v0.1.1 -m "Kenta v0.1.1"
   git push origin v0.1.1
   ```

2. GitHub Actions runs the `release.yml` workflow:
   - Downloads and verifies the pinned FFmpeg build by SHA-256.
   - Runs frontend gates (`npm ci`, `lint`, `tsc`, `build`).
   - Runs Rust gates (`cargo test --release --lib`, `cargo check --release --lib`).
   - Builds the NSIS installer (`cargo tauri build --bundles nsis`).
   - Computes `SHA256SUMS.txt`.
   - Creates a **draft** GitHub Release with the installer and checksums.

3. The release is **not published automatically**. Review the draft, test the
   installer, then publish it from the GitHub Releases page.

## Testing the draft installer

On a clean Windows machine (or Windows Sandbox):

1. Download `Kenta_<version>_x64-setup.exe` from the draft release assets.
2. Verify the SHA-256 against `SHA256SUMS.txt`:

   ```powershell
   Get-FileHash Kenta_<version>_x64-setup.exe -Algorithm SHA256
   ```

3. Run the installer (per-user, no admin rights required).
4. Launch Kenta from the Start Menu.
5. Start replay buffering, save a clip, open it in the library, and play it.
6. Verify microphone and desktop audio handling.
7. Test the editor export on a disposable clip.
8. Restart Kenta and confirm settings and library persist.
9. Install a newer build over this one and confirm data survives.
10. Uninstall and confirm recordings and the library database are not deleted.

## Publishing

Once testing is complete, publish the draft release on GitHub. Add release
notes summarizing user-facing changes. Do not publish before clean-machine
testing.

## Unsigned builds

The installer is currently unsigned. Windows SmartScreen will warn users.
Document this in `README.md` and `docs/INSTALL.md`. When a code-signing
certificate is configured, add a signing step to the CI workflow and remove the
unsigned-build notices.

## What is never committed

- `google-oauth.json` (gitignored; contains OAuth client config)
- `src-tauri/target/` (gitignored build cache)
- `web/dist/` (gitignored build output)
- `.dist/` (gitignored local runnable build)
- Installer binaries (distributed via GitHub Releases, not source history)