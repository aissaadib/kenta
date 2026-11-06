# Google Drive in Kenta

Google Drive support is optional and one-way. Kenta creates a folder named
**Kenta** in your Drive and uploads clips you choose, or new clips by default.
It never downloads, syncs, deletes, or overwrites Drive files.

## What is uploaded

- A clip you choose with **Save to Google Drive**.
- New clips after an account is connected, unless you turn off automatic
  upload in **Settings → Google Drive**.

Kenta does not upload thumbnails, settings, the clip database, logs, or audio
samples. Connecting an account never uploads your existing library.

## Disconnecting

Disconnecting stops future uploads, cancels in-flight jobs, revokes the
refresh token when Google allows it, and removes the stored credential. It
does not delete local clips or files already in Drive.

## Scopes

Kenta requests only:

- `https://www.googleapis.com/auth/drive.file` — create and manage files that
  Kenta itself creates.
- `openid email` — show the connected account's email in Settings.

It never requests full `drive` access and never lists or indexes your files.

## OAuth client

The released app uses a Google **Desktop app** OAuth client. The installer ships
the public client configuration as `google-oauth.json` next to `Kenta.exe`.
Google treats desktop clients as public clients: the client secret is not a
confidential credential and cannot be protected in a distributed binary. The
client still uses PKCE and a loopback redirect.

The lookup order is:

1. `KENTA_GOOGLE_CLIENT_ID` / `KENTA_GOOGLE_CLIENT_SECRET` (development).
2. `%APPDATA%\Kenta\google-oauth.json`.
3. `google-oauth.json` in the installation directory.
4. Google's downloaded `client_secret_*.json` in the same locations.

## Google Cloud setup (one time, for maintainers)

1. Create a project in the [Google Cloud Console](https://console.cloud.google.com/).
2. Enable the **Google Drive API**.
3. Configure the **OAuth consent screen**.
4. Create **Credentials → OAuth client ID → Desktop app**.
5. Download the JSON and save it as `google-oauth.json` in the repository root
   for a local release build. Do not commit it.

## Distribution limitation

Google limits an unverified OAuth app in **Testing** mode to accounts added as
test users. To let arbitrary users connect, the Google Cloud project must be
published (and may require Google verification depending on the scopes and
user type). This is a Google account/consent-screen step, not an application
code step. If the project remains in Testing mode, document that only test
accounts can connect.

## Token storage

- Access tokens stay in memory only.
- Refresh tokens are stored in Windows Credential Manager under
  `Kenta/google-drive/<account id>`, not in `settings.json`, the database,
  logs, or the frontend.
- No token, authorization code, or upload session URL is sent to the React
  frontend or emitted through Tauri events.
