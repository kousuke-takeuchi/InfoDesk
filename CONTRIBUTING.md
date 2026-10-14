# Contributing to InfoDesk

[日本語](CONTRIBUTING.ja.md)

Thanks for your interest. InfoDesk is a local-first desktop reader (Tauri 2 + React 19 + Rust) that collects tech and medical-care news and writes Japanese digests. It is developed **spec-first, design-first, test-first**. This page is the short version. The developer docs under [`docs/internal/`](docs/internal/README.md) (written in Japanese) are the long one.

## Ground rules

1. **Spec and plans first.** The master design is `docs/superpowers/specs/2026-10-06-infodesk-design.md`. Plans live in `docs/superpowers/plans/` (date-prefixed). If behavior changes, update the spec first.
2. **Design before UI.** Screens are designed in Pencil (`design.pen`, with exports in `design/exports/`) before they are implemented. Reuse existing tokens and components, and add new ones to the design first. Do not read `design.pen` as text; it is only accessible through the Pencil tools.
3. **TDD.** Write a failing test, make it pass, then refactor. Behavior without a test does not exist.
4. **Unit tests never touch the network.** Use fixtures (`crates/core/tests/fixtures/`), a mock HTTP server or injected clients. Real responses for fixtures are refreshed on purpose with `cargo run -p infodesk-core --example fetch_fixtures`.
5. **The UI is Japanese and there is no i18n.** Japanese UI strings are written directly in the React components. Do not add a message-catalog layer as part of an unrelated change.
6. **English is canonical for documentation**, with a Japanese twin (`*.ja.md`). New public docs are written in both and keep the same structure. Developer docs in `docs/internal/` are Japanese only.
7. **No AI attribution trailers in commits.** Do not add `Co-Authored-By:` or similar lines to commit messages.
8. **No emojis and no em-dashes** in documentation.

## Build requirements

Besides Rust and Node, building the app needs **Go** on your PATH, because the in-app Tailscale node (libtailscale) is written in Go. `.cargo/config.toml` sets `GOTOOLCHAIN=go1.25.5`, so the first build may download that toolchain, and the binary grows by about 26 MB. Without Go the build script panics with `go build command failed to start`. See [Getting Started](docs/02-getting-started.md).

## Workflow

1. Read the spec section for the area you touch.
2. For anything beyond a small fix, add a plan under `docs/superpowers/plans/` and work it task by task with red then green commits.
3. Run the checks before you commit:

   ```bash
   cargo test                                   # Rust: core, CLI and app crates
   (cd app && npx vitest run)                   # React UI
   (cd agent && npx vitest run && npm run typecheck)   # Node sidecar
   ```

   The agent script `npm test` sets `TZ=Asia/Tokyo`; use it if a date-dependent test fails.
4. Keep commits small and descriptive. Use Conventional Commit prefixes such as `feat`, `fix`, `docs`.
5. Describe which spec section or plan you followed.

## What belongs where

| Area | Path |
|---|---|
| Collection, storage (SQLite), summaries, page fetch | `crates/core` |
| `infodesk` CLI | `crates/cli` |
| Tauri shell: commands, reader window, jobs, scheduler | `app/src-tauri` |
| React UI | `app/src` |
| Chat agent (Node sidecar) | `agent/` |
| Design and exports | `design.pen`, `design/exports/` |
| Spec and plans | `docs/superpowers/` |

## Reporting problems

Report bugs and ideas in [GitHub Issues](https://github.com/kousuke-takeuchi/InfoDesk/issues). For security issues see [SECURITY.md](SECURITY.md). Please do not share personal data or API keys when you describe a problem.

## License

By contributing you agree that your contribution is licensed under MIT OR Apache-2.0, like the rest of the project (see [README](README.md#license)).
