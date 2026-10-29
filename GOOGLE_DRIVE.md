# ClipVault — Google Drive integration

ClipVault can upload your saved clips to Google Drive. Upload is one-way and
explicit: ClipVault never downloads from Drive, never syncs, and never deletes
a file on your local machine or in your Drive.

## What gets uploaded

Only the MP4 files you choose:

- A clip from the library, through **Upload to Google Drive** in its overflow
  menu.
- Future clips, only if you turn on **Automatically upload new clips** in
  Settings. This is off by default, and connecting an account does not upload
  your existing library.

ClipVault does not upload thumbnails, the settings file, the clip database,
logs, audio samples, or anything else.

## What disconnecting does and does not do

Disconnecting stops future automatic uploads, cancels in-flight jobs, revokes
the refresh token when Google allows it, and removes the stored credential.
It does not delete local clips and it does not delete files already uploaded
to Drive.

## Google Cloud setup (one time)

ClipVault uses the Google Drive API v3 through a **Desktop app** OAuth client.
Desktop clients are public clients: the client ID is not a secret, and no
client secret is bundled with the app.

1. Create a project in the [Google Cloud Console](https://console.cloud.google.com/).
2. Enable the **Google Drive API** for that project.
3. Configure the **OAuth consent screen**. For personal or internal use, the
   "Internal" user type is enough. For wider distribution, Google must verify
   the app before unrestricted use; until then the project stays in Testing
   mode and only test users can sign in.
4. Create **Credentials > OAuth client ID > Desktop app**. Note the client ID.
5. Give the client ID to ClipVault in one of two ways:
   - Set `CLIPVAULT_GOOGLE_CLIENT_ID` in the environment (development), or
   - Save the client ID as `google-oauth.json` next to `ClipVault.exe` or in
     `%APPDATA%\ClipVault\google-oauth.json`. Use `google-oauth.example.json`
     as a shape reference. This file is gitignored: never commit real credentials.

Without a configured client ID, Settings shows `Not configured` with the exact
file to create.

## Scopes requested

ClipVault requests only:

- `https://www.googleapis.com/auth/drive.file` — create and manage files that
  ClipVault itself creates. That is all an upload needs.
- `openid email` — to show the connected account's email in Settings.

The app never requests full `drive` access, never lists your Drive, and never
indexes your files. It works inside a ClipVault-owned folder named **ClipVault**
that it creates on first upload.

## Token storage

- Access tokens live in memory only and are refreshed silently before expiry.
- Refresh tokens are stored in the Windows Credential Manager under
  `ClipVault/google-drive/<account id>`, never in settings.json, the clip
  database, logs, or the frontend.
- No token, authorization code, or upload session URL is ever sent to the
  React frontend or emitted through Tauri events.

## Connecting

Connecting opens the system browser (never an embedded Google sign-in form).
You consent in the browser, Google redirects to a local loopback listener on
127.0.0.1, ClipVault exchanges the code for tokens, and the browser tab can be
closed.

## Release note

The code implements the full OAuth flow, but Google account publication and
OAuth consent-screen verification are separate release tasks. Do not claim
production verification has been completed until Google has reviewed the app.

## Requirements not yet satisfied without a real client ID

- A real Google account connection cannot be tested in an automated build
  without a configured Desktop app OAuth client.
- Until the app is verified by Google, Testing mode limits sign-in to test
  users added to the Google Cloud project.