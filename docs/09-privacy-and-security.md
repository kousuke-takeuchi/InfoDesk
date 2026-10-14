**日本語: [09-privacy-and-security.ja.md](09-privacy-and-security.ja.md)** | ← [CLI](08-cli.md) | Next: [Roadmap →](10-roadmap.md)

# Privacy and Security

InfoDesk is a single-user desktop app. It has no account and no server of its own. This page lists what stays on your Mac, what goes over the network, and which protections exist. A search of the code found no analytics or telemetry library.

To report a vulnerability, see [SECURITY.md](../SECURITY.md).

## What stays local

- The database `~/Library/Application Support/jp.infodesk.app/infodesk.db` holds sources, topics, collected articles, summaries, digests, chat threads and settings.
- Nothing is uploaded to an InfoDesk service. There is none. If you turn on [Cloud Sync](12-cloud-sync.md), a copy goes to your own Google Drive or your own NAS (below).
- With Ollama on the same Mac, summaries, digests and chat run locally. Article text does not leave the machine for them.
- Chat tools are read-only and read only this database (through the [CLI](08-cli.md)), plus pages and searches you allow (below).

## What goes over the network

| What | To where | When |
|---|---|---|
| Feeds and source APIs | The hosts of your enabled sources (the built-in ones and any RSS feed you add) | Every collection. With a 1-minute interval, every enabled source is contacted every minute and some sites may rate-limit you |
| Article pages, for `og:image` | The article's own site | At collection, for new articles that have no feed image. Up to 512 KiB per page, 10 s timeout, 4 at a time, 60 s total budget per collection |
| Article images | The image hosts | When the app shows a thumbnail or a card. The window loads them directly from `https:` |
| LLM text with Ollama | The URL in `ollama.base_url`, by default `http://localhost:11434` | Summaries, digests, chat |
| LLM text with Claude | Anthropic, through the `claude` CLI you installed and signed in | Summaries, digests, chat, when you choose Claude |
| Collected articles, digests, and read and starred marks, as files | Your own Google Drive, in the hidden app-only folder (`appDataFolder`) | Only after you sign in with Google and turn on sync. After each collection and digest, and on "sync now". See [Cloud Sync](12-cloud-sync.md) for the file list |
| The same files, as WebDAV requests | Your own Synology NAS, at the address you enter (`http://<NAS name>:5005/<folder>/`) | Only after you choose NAS, log in to Tailscale and turn on sync. They travel inside the WireGuard tunnel of the in-app Tailscale node, so the plain HTTP leg is encrypted on the network |
| Tailscale login and coordination | Tailscale (`login.tailscale.com` and its control and relay servers) | When the in-app node logs in, starts and connects. It sends what Tailscale needs to log in and connect a device, such as the device name `infodesk-mac`. It does not send your articles. Log uploads are turned off (below) |
| Google sign-in and token | Google (`accounts.google.com`, `oauth2.googleapis.com`, `www.googleapis.com`) | At sign-in, token refresh and sign-out (the token is revoked) |
| Article body for chat | The article URL | Only when the agent calls `fetch_page`. Not saved |
| Web search for chat | Claude: its built-in WebSearch. Ollama: `https://ollama.com/api/web_search` | Only when the agent searches the web. For Ollama only if you set an API key |

Notes:

- If you set `ollama.base_url` to a remote machine, your text goes to that machine. The default is local.
- With Claude, what is sent includes article titles and summaries (for summaries and digests) and your questions and tool results (for chat). Check Anthropic's terms for the CLI before you send private material. InfoDesk starts the CLI with session persistence off.
- Opening an article in the reader loads that site like a browser does. The site sees your visit.
- Requests identify themselves as `InfoDesk/0.1`.

## Stored secrets

The ollama.com API key is optional. It is stored (`ollama.web_search_api_key`) as plain text in the SQLite settings table, and `infodesk config list` prints it. Anyone who can read the database file can read the key. Keep the file permissions of your user account private, and do not share the database or screenshots of `config list`.

Claude is used through its CLI, so InfoDesk stores no Claude credential.

If you sign in for [Cloud Sync](12-cloud-sync.md), the Google refresh token is stored in the macOS Keychain (service `jp.infodesk.app.google`, account `refresh_token`), not in the database. Signing out revokes the token at Google and deletes the Keychain item. The app asks Google for one Drive permission, `https://www.googleapis.com/auth/drive.appdata`, which reaches only InfoDesk's own hidden folder and none of your other Drive files (it also asks for `openid` and `email`, to show your address). The OAuth client ID and secret are compiled into the app; for a desktop app Google does not treat the secret as confidential. Sign-in uses PKCE and a one-time listener on `127.0.0.1`.

If you choose NAS storage (see [NAS Setup](13-nas-setup.md)):

- The NAS is reached only through the Tailscale node inside InfoDesk. There is no system VPN, so other apps on the Mac are not affected, and no Tailscale app is needed.
- Tailscale log uploads to `log.tailscale.com` are turned off with the environment variable `TS_NO_LOGS_NO_SUPPORT`. Because the Go runtime inside the app reads its environment at load, InfoDesk restarts itself once at launch with the variable set. If that restart fails, Tailscale is not started.
- The Tailscale state (the node key) is saved in the `tailscale/` folder of the app data directory, with mode 0700. Pressing "ログアウト" in the Sync tab removes the device from your tailnet. "この Mac の Tailscale の状態を消す" deletes the saved state on this Mac.
- The NAS password is stored in the macOS Keychain (service `jp.infodesk.app.nas`), not in the database. The database keeps only whether a password is set.
- Sync data goes to the NAS over HTTP with Basic authentication, inside the WireGuard tunnel.

## Page fetch guard (SSRF)

Chat and the CLI can fetch URLs that come from article text, which the model may follow. A page that tells the agent to fetch an internal address must not work. `crates/core/src/page.rs` therefore applies these rules to `fetch_page`, `infodesk page` and the `og:image` fetch:

- Only `http` and `https` URLs.
- Addresses are checked at name resolution, so a DNS name that points to a private address is refused. IP literals and `localhost` names are checked before connecting.
- Refused ranges: loopback, private, link-local, unspecified, multicast, broadcast, `0.0.0.0/8`, carrier-grade NAT `100.64.0.0/10`, IPv6 unique local `fc00::/7` and link-local `fe80::/10`. IPv4-mapped IPv6 addresses are treated as their IPv4 address.
- Every redirect is checked again, with at most 5 redirects.
- At most 2,000,000 bytes are read, with a 30 s timeout. Body text is cut to the character limit.
- The fetched page text is not stored in the database.

This guard is for the agent and CLI fetches. The feed collector fetches the sources you configured.

## Reader isolation

The in-app reader shows an article inside the app, so it is built to keep an outside page away from the app's own powers.

- The reader window has two webviews: a toolbar (an app page) and the article page. Only the main window and the toolbar webview are listed in `app/src-tauri/capabilities/default.json`. The article webview has no capability, so a page in it cannot call the app's IPC commands.
- The permissions that file grants to the app's own pages are `core:default`, `core:event:default`, showing, hiding and focusing windows, `opener:default` and `autostart:default`.
- The reader opens only `http` and `https` URLs, and refuses local hosts: `localhost` and `*.localhost`, and loopback, private, link-local, unspecified and multicast addresses. These can overlap the app's own origin, and loading them would give the page access to IPC. The same check applies to navigation inside the reader.
- A page that tries to open a new window is denied. If its target is allowed, the reader navigates in place instead.
- Local and LAN article links are opened in your default browser, not in the reader.
- Links in chat answers open in the reader only for `http` and `https`. Images in answers are not loaded.

## Content Security Policy

The app's pages run under this policy (`app/src-tauri/tauri.conf.json`):

```
default-src 'self'; connect-src ipc: http://ipc.localhost http://127.0.0.1:*; img-src 'self' data: https:; style-src 'self' 'unsafe-inline'; font-src 'self' data:
```

- Scripts and everything else default to the app's own files.
- Network requests from the app's pages can go only to the Tauri IPC and to `127.0.0.1` (the chat agent).
- Images may load from `https:` hosts, for article thumbnails. This is why chat answers drop images: a remote image in an answer would make a request just by being shown.

## The chat agent sidecar

Chat runs in a separate Node process that the app starts. It is not a public server.

- It listens on `127.0.0.1` only, on a free port chosen at start.
- Each time the app starts the sidecar, it creates a random 16-byte token (32 hex characters, read from `/dev/urandom`) and passes it to the sidecar and to the app's page. Every request must send `Authorization: Bearer <token>`. The comparison is constant-time. A wrong or missing token gets 401.
- CORS accepts only the origins `tauri://localhost` and `http://localhost:1420` (the development server). A request from any other origin gets 403.
- Only `GET /health` and `POST /chat` exist. Request bodies over 5 MiB are refused (413), and an invalid body gets 400.
- The sidecar reads your data by running the `infodesk` CLI against the app database. It has no write tool.
- When the app quits or crashes, the sidecar's standard input closes and it exits.

## Summary of limits

- The API key is stored in plain text (see above).
- With sync on, your articles and digests are copied to your Google Drive. Anyone with access to your Google account can read them there. With NAS storage, they are copied to your NAS, and anyone who can sign in to that NAS user or to your Tailscale account can reach them.
- Prompt injection cannot be fully prevented. See [AI Chat](06-ai-chat.md).
- Opening articles and loading images contacts third-party sites.
- The app is alpha software for macOS. It has had no external security review.
