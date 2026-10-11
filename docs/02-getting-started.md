**日本語: [02-getting-started.ja.md](02-getting-started.ja.md)** | ← [Overview](01-overview.md) | Next: [Sources and Topics →](03-sources-and-topics.md)

# Getting Started

InfoDesk runs on macOS only. Apple Silicon is the tested setup. There is no prebuilt download, so you build it from source.

## 1. Prerequisites

| Software | Required | Get it | Check |
|---|---|---|---|
| Xcode Command Line Tools | yes | `xcode-select --install` | `xcode-select -p` |
| Rust 1.89 or later (rustup) | yes | https://rustup.rs | `cargo --version` |
| Node.js 22 or later | yes | https://nodejs.org | `node --version` |
| Ollama | one of the two | https://ollama.com | `ollama list` |
| Claude Code CLI (`claude`) | one of the two | https://claude.com/claude-code | `claude --version` |

- **You need at least one LLM provider.** The default is Ollama with the model `gemma4:26b` (run `ollama pull gemma4:26b`). You can choose another model in Settings. The `claude` CLI works as an alternative for summaries and chat.
- **Node.js** is used by the chat agent, a small sidecar process that the app starts. The collector and the digests do not need it. Without Node the app still works, and only chat is unavailable.
- rustup installs `cargo` into `~/.cargo/bin`. Add it to your PATH if needed.

## 2. Run in development

```bash
git clone <this repository>
cd InfoDesk
cd agent && npm install && npm run build && cd ..   # chat agent (skip if you do not need chat)
cd app && npm install
npm run tauri dev
```

This starts Vite and builds the Rust side, then opens the app window.

Run the tests:

```bash
cargo test                # Rust crates, no network access
cd app && npm test -- --run   # UI tests (plain npm test starts watch mode)
```

## 3. Build the app bundle

```bash
cd app && npm run bundle
```

This builds the agent, then runs `tauri build --debug --bundles app`. The result is a debug build of the app at `target/debug/bundle/macos/InfoDesk.app` (the `target` directory at the repository root).

## 4. Install to /Applications

```bash
cp -R target/debug/bundle/macos/InfoDesk.app /Applications/
```

Run it from the repository root. Then open InfoDesk from Launchpad or Finder. The bundle identifier is `jp.infodesk.app`.

## 5. First launch

1. The app creates its database at `~/Library/Application Support/jp.infodesk.app/infodesk.db`.
2. On the first run only, it adds the built-in sources and the default settings. See [Sources and Topics](03-sources-and-topics.md).
3. The first collection starts on its own right after launch, because nothing has been collected yet. You can also press **Collect now** (the button in the empty feed, or in the menu bar). A first collection takes one to two minutes, and progress shows at the bottom of the sidebar.
4. Summaries are attached to the top articles of each category, then the digest for today is built. Open the **Digest** view to read it.
5. After that, collection runs on a schedule (every 3 hours by default), and the daily digest is finalized at 07:00. The schedule is described in [Sources and Topics](03-sources-and-topics.md#5-collection-schedule).

The app keeps running in the menu bar when you close the window. The "stay resident" setting turns this off.

## 6. Troubleshooting

| Symptom | Cause and fix |
|---|---|
| `cargo: command not found` | rustup is missing or not on PATH: `export PATH="$HOME/.cargo/bin:$PATH"` |
| Digests stay "pending" or become "failed" | The LLM is not reachable. See the next two rows, then press **Retry now** on the digest |
| "Ollama に接続できません" (cannot connect to Ollama) | Ollama is not running. Start the Ollama app or run `ollama serve`. Check the URL in Settings (default `http://localhost:11434`) |
| "モデルが見つかりません" (model not found) | The model is not installed. Run `ollama pull <model>` for the model named in Settings (default `gemma4:26b`) |
| "Node が見つかりません" (Node not found) | Install Node 22 or later, or set the Node path in Settings, LLM tab. The app also looks in `/opt/homebrew/bin` and `/usr/local/bin` |
| Chat does not start in dev mode | Build the agent first: `cd agent && npm install && npm run build` |
| Reset everything | Quit the app and delete `~/Library/Application Support/jp.infodesk.app/infodesk.db`. Sources and settings are seeded again on the next launch |

The command line tool uses a different default database path from the app. See the CLI page for `--db`.
