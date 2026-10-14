**日本語: [12-cloud-sync.ja.md](12-cloud-sync.ja.md)** | ← [Keyboard Shortcuts](11-keyboard-shortcuts.md) | Next: [NAS Setup →](13-nas-setup.md)

# Cloud Sync

InfoDesk can copy what it collects to your own Google Drive, so that a phone app can read it. This page explains what sync does, how to use it, the one-time Google setup a developer must do before building the app, what is stored on Drive, and what to do when something goes wrong.

The mobile app itself is not part of this release. This release contains the Mac side only.

## 1. What it does

- The Mac collects articles and makes digests, as before. After that, it writes them to a hidden, app-only area of your Google Drive.
- A phone reads those files. It never writes articles or digests.
- Read and starred marks go both ways. A mark made on the phone reaches the Mac, and a mark made on the Mac reaches the phone. When two devices change the same article, the newer change wins.
- Each file on Drive has one writer, so devices never overwrite each other.
- If the network is down, the Mac keeps working on its own and catches up at the next sync.

## 2. Choosing where to sync

The Sync tab has a "保存先" (storage) choice with two options:

- **Google ドライブ** (the default): the hidden app-only area of your Google Drive, as described in this page.
- **NAS (WebDAV)**: a Synology NAS you own, reached through a Tailscale node inside InfoDesk. See [NAS Setup](13-nas-setup.md).

The files and the rules (retention, one writer per file, read and starred marks) are the same for both. Switching the storage does a full re-publish to the new storage on the next sync. The old storage keeps its files; InfoDesk does not delete them.

## 3. How to use it (Google Drive)

1. Open Settings (Cmd+,) and select the "同期" (Sync) tab.
2. Press "Google でログイン". Your browser opens a Google page. Sign in and allow access. You do not type any ID or password into InfoDesk. When the browser says you are signed in, return to InfoDesk.
3. The tab now shows your account. Turn on "同期する" if it is off (it is off by default).
4. Press "今すぐ同期" to sync at once.

After that, a sync runs by itself:

- after each collection (this includes Cmd+R, which collects)
- after a digest is generated
- when you press "今すぐ同期"

Only one sync runs at a time. The tab shows the time of the last sync and, on a separate line, the last sync error. A sync error never affects collection or digests. You can also change the name of this Mac (the default is the computer name), which appears in the sync records.

"ログアウト" revokes the access token at Google and removes the stored token from the macOS Keychain.

![The Sync tab (design mockup)](../design/exports/13-settings-sync.png)

If this build has no Google credentials (section 4), the tab shows "このビルドでは同期を使えません" and the sign-in button is hidden.

## 4. For developers: Google setup

Users of a ready-made build skip this section. Do it once if you build InfoDesk yourself. The OAuth client ID and secret are not in the repository, so a build without them has sync disabled.

The Cloud Console changes its screens from time to time, so this guide names sections generically. Look for the matching item if a label differs.

1. **Create a project.** Sign in to the Google Cloud Console with a personal Google account and create a new project (for example "InfoDesk").
2. **Enable the Google Drive API.** In the project, open "APIs & Services", then the library, find "Google Drive API" and enable it.
3. **Configure the OAuth consent screen.** Under "APIs & Services", open the OAuth consent screen (Google may group it under "Google Auth Platform").
   - App type or audience: external.
   - Scopes: add only `https://www.googleapis.com/auth/drive.appdata` as a Drive scope. Add no other Drive or sensitive scope. (The app also requests the basic `openid` and `email` identity scopes, only to show your address.)
   - Set the publishing status to "In production".
4. **Create an OAuth client.** Under "APIs & Services > Credentials", create an OAuth client ID with application type "Desktop app". Copy the client ID and the client secret.
5. **Put the values in the build.** Copy `app/src-tauri/google-oauth.example.json` to `app/src-tauri/google-oauth.json` and fill in the two values:

   ```json
   {
     "client_id": "YOUR_CLIENT_ID.apps.googleusercontent.com",
     "client_secret": "YOUR_CLIENT_SECRET"
   }
   ```

   You can also save the JSON file that the console offers for download (its values are under `"installed"`) as `app/src-tauri/google-oauth.json` as it is.

6. **Rebuild** the app (`cd app && npm run bundle`, see [Getting Started](02-getting-started.md), or `npm run tauri dev`). The values are read at build time and compiled in. If you edit the file, rebuild.

Notes:

- **Why "In production".** While the publishing status is "Testing", Google expires refresh tokens after 7 days, and you would have to sign in again every week. "In production" avoids that.
- **No verification needed.** `drive.appdata` is a non-sensitive scope, so Google does not require an app review for it.
- **The file is git-ignored.** `app/src-tauri/google-oauth.json` is listed in `.gitignore`. Do not commit it. For a desktop app, Google does not treat the client secret as confidential, but it still should not go into a public repository.
- **A missing or broken file** disables sync. A file that is not valid JSON also prints a build warning.
- The same Google Cloud project is meant to serve the future mobile client (an iOS OAuth client), because the app-only Drive folder is shared per project.

## 5. What is stored on Drive

InfoDesk uses the hidden app data folder of your Drive (`appDataFolder`). The files are not shown in the normal Drive file list. To see that InfoDesk holds hidden data, open your Google account's Drive settings and look at "Manage apps".

| File | Written by | Content |
|---|---|---|
| `manifest.json` | Mac | Format version, database id, the name, hash and update time of each file, last write time |
| `meta.json.gz` | Mac | Categories, topics, sources |
| `day-YYYY-MM-DD.json.gz` | Mac | Articles collected that day (title, excerpt, AI summary, image URL, source, category, score, times, matching topic ids) |
| `digest-{day,week,month}-YYYY-MM-DD.json.gz` | Mac | The digests, with the period overview |
| `state-snapshot.json.gz` | Mac | The settled read and starred state, made about once a day |
| `ops-<device id>-<time>-<number>.json` | each device | Batches of read and starred changes. Written once, never edited |

Read and starred marks are not stored inside the article files. Article images are not stored on Drive; the phone loads them from the original sites.

Retention:

- Articles and daily digests: the last 60 days (Mac dates). Older `day-*` and `digest-day-*` files are deleted by the Mac.
- Weekly and monthly digests: the last 400 days.
- Once a day the Mac folds all change batches into `state-snapshot.json.gz` and deletes the batches it covered. Uploaded change records on the Mac older than 30 days are also pruned.
- The first sync, and the first sync after you turn "同期する" on again, always writes `state-snapshot.json.gz`. It carries the read and starred marks you set before, including marks set before sync existed or while sync was off. While sync is off, the Mac keeps each mark's time but does not record change batches to send.
- The phone keeps its local copy until you delete it from its settings.

If you reset the Mac database, its article ids change. On its next sync the Mac sees the old database id in `manifest.json`, deletes all change batches and the old `state-snapshot.json.gz` without applying them (their article ids point at other articles), and publishes the new database. Marks made on the phone that the Mac had not yet imported are lost. The new database id in `manifest.json` tells the phone to discard its copy and fetch again.

## 6. Troubleshooting

- **"Google の再ログインが必要です" (re-login needed).** The stored token no longer works (you revoked access in your Google account, the password changed, or the token expired). Sync stops. Press "ログアウト" if shown, then "Google でログイン" again.
- **Sign-in never finishes.** The browser must be able to reach `http://127.0.0.1` on your own Mac, because Google sends the result back there. The sign-in waits for 5 minutes, then gives up. Press the button again.
- **Drive is full.** The Mac's files are small (compressed JSON), but app data counts against your Drive storage. If Google reports a storage error, the sync fails and shows the error. Free space in your Google account and press "今すぐ同期".
- **Rate limits and network errors.** Network errors, HTTP 429, 5xx and Drive rate limit errors stop that sync. The next sync retries. Nothing is lost. The sync line of the Sync tab shows "ネットワークに接続できません。次の同期で再試行します。" (cannot reach the network; the next sync retries).
- **"このビルドでは同期を使えません".** The build has no `google-oauth.json`. See section 4.
- **A phone mark does not show up on the Mac.** Marks are imported at each sync. Press "今すぐ同期" on the Mac.
- To stop sync, turn off "同期する", or press "ログアウト". Files already on Drive stay there until you remove them from Drive's "Manage apps" settings.

For NAS problems, see the troubleshooting table in [NAS Setup](13-nas-setup.md).

See [Settings](07-settings.md) for the settings keys and [Privacy and Security](09-privacy-and-security.md) for what is sent to Google.
