# Kenta — Google Drive integration

Kenta can upload your saved clips to Google Drive. Upload is one-way and
explicit: Kenta never downloads from Drive, never syncs, and never deletes
a file on your local machine or in your Drive.

## What gets uploaded

Only the MP4 files you choose:

- A clip from the library, through **Save to Google Drive** in its preview or
  overflow menu.
- New clips, automatically. Kenta saves the local file first and then copies it
  to Drive in the background, so a normal save exists in both places. This is on
  by default for future clips only: connecting an account never uploads your
  existing library. You can turn it off in Settings.

Kenta does not upload thumbnails, the settings file, the clip database,
logs, audio samples, or anything else.

## What disconnecting does and does not do

Disconnecting stops future automatic uploads, cancels in-flight jobs, revokes
the refresh token when Google allows it, and removes the stored credential.
It does not delete local clips and it does not delete files already uploaded
to Drive.

## Google Cloud setup (one time)

Kenta uses the Google Drive API v3 through a **Desktop app** OAuth client.
The client ID is not a secret, but Google still requires the client secret
when exchanging an authorization code. The secret is not confidential; it just
has to be present in the configuration.

1. Create a project in the [Google Cloud Console](https://console.cloud.google.com/).
2. Enable the **Google Drive API** for that project.
3. Configure the **OAuth consent screen**. For personal or internal use, the
   "Internal" user type is enough. For wider distribution, Google must verify
   the app before unrestricted use; until then the project stays in Testing
   mode and only test users can sign in.
4. Create **Credentials > OAuth client ID > Desktop app**. Note the client ID
   and client secret.
5. Give both values to Kenta in one of two ways:
   - Set `KENTA_GOOGLE_CLIENT_ID` and `KENTA_GOOGLE_CLIENT_SECRET` in the
     environment (development), or
   - Save them as `google-oauth.json` next to `Kenta.exe` or in
     `%APPDATA%\Kenta\google-oauth.json`. You can also save Google's downloaded
     `client_secret_*.json` there unchanged. Use `google-oauth.example.json`
     as a shape reference. This file is gitignored: never commit real credentials.

Without a configured client ID and secret, Settings shows `Not configured`
with the exact file to create. If the secret is missing, the message says so
instead of opening a browser that can only end in an error.

## Scopes requested

Kenta requests only:

- `https://www.googleapis.com/auth/drive.file` — create and manage files that
  Kenta itself creates. That is all an upload needs.
- `openid email` — to show the connected account's email in Settings.

The app never requests full `drive` access, never lists your Drive, and never
indexes your files. It works inside a Kenta-owned folder named **Kenta**
that it creates on first upload.

## Token storage

- Access tokens live in memory only and are refreshed silently before expiry.
- Refresh tokens are stored in the Windows Credential Manager under
  `Kenta/google-drive/<account id>`, never in settings.json, the clip
  database, logs, or the frontend.
- No token, authorization code, or upload session URL is ever sent to the
  React frontend or emitted through Tauri events.

## Connecting

Connecting opens the system browser (never an embedded Google sign-in form).
You consent in the browser, Google redirects to a local loopback listener on
127.0.0.1, Kenta exchanges the code for tokens, and the browser tab can be
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