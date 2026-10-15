**日本語: [07-settings.ja.md](07-settings.ja.md)** | ← [AI Chat](06-ai-chat.md) | Next: [CLI →](08-cli.md)

# Settings

Open Settings from the sidebar, or from the menu bar item "設定…" (Cmd+,). Settings has five tabs: Sources, Topics, LLM, Schedule and Sync. Changes are saved when you edit a field.

## Tabs

| Tab | What it holds |
|---|---|
| Sources | Turn built-in sources on or off, add RSS feeds. See [Sources and Topics](03-sources-and-topics.md). |
| Topics | Topics and their keywords. See [Sources and Topics](03-sources-and-topics.md). |
| LLM | Which LLM each job uses, the Ollama and Claude settings, the Node path, connection tests. |
| Schedule | Collection interval, digest times, launch behavior, retry of pending digests. |
| Sync | Choose the storage (Google Drive or NAS), sign in with Google or Tailscale, set the NAS address and user, turn sync on or off, this Mac's name, last sync time and error, "sync now". See [Cloud Sync](12-cloud-sync.md). |

### LLM tab

![The LLM tab (sample data)](images/settings-llm.png)

*The LLM tab. Sample data from a mocked backend.*

- "要約に使うLLM": choose Ollama or Claude (claude CLI) separately for article summaries, period digests and chat. Choosing Claude does not fall back to Ollama when it fails.
- "Ollama": the URL, the model (a menu of the models installed in Ollama, with a "再読み込み" button to refetch; if Ollama cannot be reached it becomes a text box, still with the button), a connection test, and the web search API key for chat.
- "Claude": the path of the `claude` command, the model (`sonnet`, `opus`, `fable`, or "その他 (名前を入力)" to type a full model name), a connection test, and the path of Node (used to run the chat agent).
- The path fields accept a bare command name (letters, digits, `_`, `.`, `-`) or an absolute path to an existing file. A GUI app does not inherit your shell PATH, so an absolute path is the safe choice.

### Schedule tab

![The Schedule tab (sample data)](images/settings-schedule.png)

*The Schedule tab. Sample data from a mocked backend.*

- "収集": the collection interval (1, 5, 15 or 30 minutes, or 1, 3, 6 or 12 hours, in the menu), the next collection time, and a "collect now" button.
- "要約の生成": the daily time, the weekly day and time, and the monthly time (on the 1st). At the daily time the previous day's digest is finalized and the current week and month are rebuilt. Today's daily digest is not rebuilt by collection; make it with **Generate** or **Regenerate**.
- "起動": launch at login and stay resident.
- "保留中の要約": the last error, and a button to retry digests that are pending or failed. The scheduler also retries pending digests on its own, at most once every 15 minutes, whether or not other jobs ran. Failed digests are retried only with this button or the one on the digest.

A job that did not run (the Mac was asleep, or the app was closed) is made up once at the next start or wake.

## Settings keys

All settings are stored in the app database as text. Defaults are written on first launch. A missing key falls back to its default.

| Key | Default | Meaning |
|---|---|---|
| `llm.items_provider` | `ollama` | Provider for article summaries. `ollama` or `claude`. |
| `llm.digest_provider` | `ollama` | Provider for day, week and month digests. `ollama` or `claude`. |
| `llm.chat_provider` | `ollama` | Default provider for chat. `ollama` or `claude`. The chat panel can switch per panel. |
| `ollama.base_url` | `http://localhost:11434` | Ollama server URL. |
| `ollama.model` | `gemma4:26b` | Ollama model name. |
| `ollama.web_search_api_key` | empty | ollama.com API key. Enables web search in chat when the provider is Ollama. |
| `claude.bin` | `claude` | Command or absolute path of the Claude CLI. |
| `claude.model` | `sonnet` | Claude model passed to the CLI as `--model`: `sonnet`, `opus`, `fable` or a full model name. Used for summaries, digests and chat. Empty means the CLI default for summaries and `sonnet` for chat. Only letters, digits and `. _ : - [ ]` are allowed, and it cannot start with `-`. |
| `node.bin` | `node` | Command or absolute path of Node (22 or newer) for the chat agent. |
| `summary.top_n` | `20` | How many top articles per category get an article summary after each collection. Not shown in Settings. Must be a number. |
| `schedule.collect_interval_minutes` | `180` | Minutes between collections. Values outside 1 to 10080 are clamped. An invalid value falls back to 180. |
| `schedule.collect_interval_hours` | `3` | Legacy. Read only when the minutes key is missing (clamped to 1 to 168 hours). On launch, a missing minutes key is seeded from this value. Not shown in Settings. |
| `schedule.daily_time` | `07:00` | Time (HH:MM) to finalize the previous day's digest. |
| `schedule.weekly_day` | `mon` | Day of the weekly digest: `mon`, `tue`, `wed`, `thu`, `fri`, `sat` or `sun`. |
| `schedule.weekly_time` | `07:30` | Time (HH:MM) of the weekly digest. |
| `schedule.monthly_time` | `08:00` | Time (HH:MM) of the monthly digest, on the 1st of the month. |
| `app.launch_at_login` | `false` | Start InfoDesk at login, in the menu bar without opening a window. |
| `app.stay_resident` | `true` | Keep running in the menu bar when the main window is closed. |
| `sync.enabled` | `false` | Sync to the chosen storage (Google Drive or NAS) after collections and digests. Needs a Google sign-in, or a Tailscale login and NAS address. See [Cloud Sync](12-cloud-sync.md). |

An invalid time or weekday value is ignored and the default is used.

The scheduler also keeps its own bookkeeping keys: `schedule.last_collect_at`, `schedule.last_daily_for`, `schedule.last_weekly_for` and `schedule.last_monthly_for`. They record what already ran. Do not edit them.

Sync keeps its own keys too: `sync.device_name` (the name of this Mac, edited in the Sync tab), `sync.device_id` (a random id made on the first read or star change, or on the first sync, whichever comes first), `sync.db_id` (a random id made on the first sync), `sync.account_email`, `sync.last_sync_at` and `sync.last_compaction_at`. The storage choice adds more keys: `sync.backend` (`gdrive`, the default, or `nas`; any other value is read as `gdrive`), `sync.nas.url`, `sync.nas.user`, `sync.nas.password_set` (only records that a password was saved) and `sync.reset_pending` (set when the storage changes, so the next sync starts over on the new storage). The NAS password itself is in the macOS Keychain (service `jp.infodesk.app.nas`), and the Tailscale state is in the `tailscale/` folder of the app data directory. See [NAS Setup](13-nas-setup.md). Only `sync.enabled` is a plain setting; the Sync tab manages the others. Do not edit them. The Google refresh token is not in the database; it is in the macOS Keychain.

The app only accepts the keys above. Setting `app.launch_at_login` is done with the toggle, because it also registers the macOS LaunchAgent. With the [CLI](08-cli.md), `config set` writes any key without checking it, so type carefully.

## Menu bar

InfoDesk puts an icon in the macOS menu bar. Clicking it opens this menu:

- A status line: the last collection time ("最終収集 HH:MM"), "収集中…" while collecting, or "未収集".
- An unread count and the state of today's digest, for example "未読 12件 · 日次要約 生成済み".
- "今すぐ収集": collect now.
- "ウィンドウを開く" (Cmd+O): show the main window.
- "設定…" (Cmd+,): open Settings.
- "InfoDesk を終了" (Cmd+Q): quit.

## Stay resident and launch at login

- **Stay resident** (`app.stay_resident`, on by default): closing the main window hides it. Collection and digest jobs keep running in the menu bar. Turn it off to quit when the window closes. The reader window always closes normally.
- **Launch at login** (`app.launch_at_login`, off by default): starts InfoDesk when you log in, in the menu bar, with no window. It registers a macOS LaunchAgent that starts the app with `--hidden`.
- To quit completely, use "InfoDesk を終了" in the menu bar.
