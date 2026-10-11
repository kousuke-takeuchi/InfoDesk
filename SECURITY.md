# Security Policy

## Reporting a vulnerability

This repository has no public issue tracker yet. Please do not open a public report. Contact the project owner (Kousuke Takeuchi) through a private channel instead. Include the version (or commit), the macOS version, and steps to reproduce. Please do not attach personal data or API keys.

## What is in scope

- The desktop app (Tauri shell and React UI) and the `infodesk` CLI
- The chat agent sidecar, which the app starts and which listens on `127.0.0.1`
- The in-app reader window that shows article pages
- Handling of the Ollama API key (`ollama.web_search_api_key`) stored in the settings

## Security design notes

- **SSRF-guarded page fetch.** The fetch used for article images and the agent's page tool accepts only `http` and `https`, resolves names through a resolver that refuses loopback, private and link-local addresses, checks every redirect (at most 5), and caps the body at 2,000,000 bytes (`crates/core/src/page.rs`).
- **Reader isolation.** The reader window loads remote article pages only for `http` and `https` URLs whose host is not localhost, a private address or a link-local address. These hosts could overlap the app's own origins. The default capability grants no `remote` access, so article pages cannot call the app's IPC (`app/src-tauri/src/reader.rs`, `app/src-tauri/capabilities/default.json`). Local and LAN links open in the default browser instead.
- **Sidecar bearer token.** The app generates a random token per launch and passes it to the sidecar in the environment. The sidecar binds to `127.0.0.1`, requires `Authorization: Bearer <token>` (compared with `timingSafeEqual`) and rejects browser requests whose `Origin` is not the app's (`agent/src/server.ts`, `agent/src/main.ts`, `app/src-tauri/src/sidecar.rs`).
- **Content security policy.** The app's CSP limits `connect-src` to IPC and `http://127.0.0.1:*` (`app/src-tauri/tauri.conf.json`).
- **Prompt injection.** The chat agent reads external text (feed articles, fetched pages). Hidden instructions in that text can influence which URLs it fetches or what it searches. The agent writes nothing and cannot reach settings or API keys. Avoid typing secrets into the chat. See [docs/09-privacy-and-security.md](docs/09-privacy-and-security.md).

Supported versions: the latest commit on the main branch (the project is alpha, version 0.1.0).
