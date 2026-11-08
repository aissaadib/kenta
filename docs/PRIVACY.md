# Privacy and local data

Kenta is local-first. It does not include telemetry, analytics, crash
reporting, advertising, or an account system.

## What stays on your PC

- Video clips and the SQLite library database.
- Settings, hotkeys, capture preferences, and the output folder.
- The active appearance theme, theme assets, and music playlist indexes.
- Thumbnails and temporary replay segments.
- The Google Drive refresh token, stored in Windows Credential Manager under
  `Kenta/google-drive/<account id>`.

Kenta does not upload the database, settings, themes, thumbnails, logs, music,
or audio samples. Music files are never copied: the library stores their
existing path, and playback reads the file in place.

## Google Drive

Google Drive is optional and one-way. When you connect an account:

- Kenta creates or opens a folder named **Kenta** in your Drive.
- It can upload clips you choose, and new clips by default.
- It never downloads, syncs, deletes, or overwrites files.
- It requests `openid`, `email`, and `drive.file`. `drive.file` limits access
  to files created by Kenta.
- Access tokens stay in memory. The refresh token is kept in Windows
  Credential Manager, not in the database or settings file.

Disconnecting revokes the refresh token when Google allows it, removes the
stored credential, and stops future uploads. It does not delete local clips or
files already in Drive.

## Network access

Kenta makes network requests only for:

- Google OAuth and Google Drive uploads, when you connect an account.
- Opening documentation or release links in your browser.

## Removing your data

Uninstalling Kenta does not delete user data. To remove everything, uninstall
the app and then delete:

- `%APPDATA%\Kenta`
- `%USERPROFILE%\Videos\Kenta`
- The clip folder you chose
- The `Kenta/google-drive/...` entries in Windows Credential Manager
- The **Kenta** folder in Google Drive, if you want to remove the cloud copy

Your music files are untouched by Kenta and are not part of its data folder.
