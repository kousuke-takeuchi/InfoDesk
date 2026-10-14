**日本語: [10-roadmap.ja.md](10-roadmap.ja.md)** | ← [Privacy and Security](09-privacy-and-security.md) | Next: [Keyboard Shortcuts →](11-keyboard-shortcuts.md)

# Roadmap

InfoDesk is an alpha, personal project at version 0.1.0. This page records what is built, what is known to be limited, and some ideas. Ideas are not promises. There is no schedule.

The design spec and the implementation plans are in `docs/superpowers/`.

## Done

The work was done in plans. Each line below is one plan.

- **Phase 0, screen design**: all screens designed in `design.pen` and confirmed before any app code.
- **Plan 1, core and CLI**: the `infodesk-core` library (collector, SQLite store with full-text search, LLM providers, article summaries and hierarchical period digests) and the `infodesk` CLI. See [CLI](08-cli.md).
- **Plan 2a, app backend**: the Tauri app state, commands, collection and summary jobs, the scheduler with catch-up, the menu bar item and launch at login. See [Settings](07-settings.md).
- **Plan 2b, app UI**: the React screens for the digest, feed and settings, with light and dark themes. See [Digests](04-digests.md) and [Feed and Reader](05-feed-and-reader.md).
- **Plan 3, chat**: the right-hand chat panel, the Node agent sidecar, threads, context chips, the Ollama and Claude providers, and the tools. See [AI Chat](06-ai-chat.md).
- **Plan 4, thumbnails and progress**: article images (feed image, then `og:image`) shown in the feed, and the chat progress display (plan, thinking, tool steps, elapsed time).
- **Plan 5, visual digest and reader**: the period overview (bars, daily trend, topic strength), headline and numbered points with image cards, and the in-app reader window with its toolbar.
- **Plan 6, shortcuts and freshness**: keyboard shortcuts with an in-app list (`?`), collection intervals down to 1 minute, daily digests made once a day (collection no longer rebuilds today's digest), and a refetch when the window regains focus or the date changes. See [Keyboard Shortcuts](11-keyboard-shortcuts.md).
- **Plan 7A, cloud sync (Mac side)**: a sync engine that writes articles, digests and read and starred changes to the hidden app folder of your Google Drive, Google sign-in (OAuth with PKCE, token in the macOS Keychain), a Sync tab in Settings, and database migration v5. See [Cloud Sync](12-cloud-sync.md).
- **Plan 7A2, NAS storage**: a choice between Google Drive and a Synology NAS (WebDAV) as the sync storage, an in-app Tailscale node that reaches the NAS without a system VPN, NAS credentials in the Keychain, and an in-app setup guide. See [NAS Setup](13-nas-setup.md).

## Known limits

- **macOS only.** Apple Silicon is the tested platform. Windows and Linux are not supported.
- **Two LLM providers.** Ollama and the Claude CLI. There is no direct API-key provider.
- **Summaries use feed text.** Collection stores the titles and descriptions that feeds and APIs return. It does not fetch article bodies. Only the chat agent fetches a body, on request.
- **Keyword matching only.** Search and topics match keywords. There is no semantic search.
- **Plain-text API key.** The ollama.com key is stored unencrypted in the database. See [Privacy and Security](09-privacy-and-security.md).
- **Chat is read-only and bounded.** The agent cannot change your data. Each answer is limited to 20 steps (Ollama) or 20 turns (Claude), and Ollama chat uses a 16384 token context.
- **Chat history cannot be renamed in the UI.** Titles come from the first question. (The store supports renaming, the panel does not offer it.)
- **Claude has no automatic fallback.** If Claude fails, nothing switches to Ollama by itself.
- **Single user.** No multiple users. Sync writes to Google Drive or a Synology NAS, and the mobile app is not built yet (Plan 7B).
- **Sync needs your own Google credentials.** A build without `app/src-tauri/google-oauth.json` has Google Drive sync disabled. See [Cloud Sync](12-cloud-sync.md).
- **Building needs Go.** The in-app Tailscale node is written in Go. See [Getting Started](02-getting-started.md).
- **No notifications.** The menu bar shows status and unread counts, but InfoDesk does not send push notifications.
- **Test data in screenshots.** The images in this documentation use sample data from a mocked backend.

## Ideas

These are ideas only. None is planned or scheduled.

- Semantic search with an embedding model such as `bge-m3`, if keyword matching proves not enough. The spec names this as a later option.
- Keeping the ollama.com API key in the macOS Keychain instead of the database.
- Other chat providers beyond Ollama and the Claude CLI.
- Windows or Linux builds. The spec scoped the first version to macOS.
- Notifications when a digest is ready. The spec scoped the first version without them.
