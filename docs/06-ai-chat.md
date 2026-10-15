**日本語: [06-ai-chat.ja.md](06-ai-chat.ja.md)** | ← [Feed and Reader](05-feed-and-reader.md) | Next: [Settings →](07-settings.md)

# AI Chat

The chat panel lets you ask an agent about the articles and digests InfoDesk has collected. The agent answers in Japanese, reads your data with a few tools, and shows what it did.

![The chat panel next to the daily digest (sample data)](images/chat.png)

*The chat panel open beside the digest. Sample data from a mocked backend.*

## Opening the chat

- Click the chat button in the toolbar to open or close the panel. It sits on the right of the Digest and Feed views.
- While Settings is open, the panel is hidden but kept, so an answer in progress and the open thread are not lost.
- If the agent cannot start (for example Node is missing), the panel shows the cause and a button to open the LLM settings. The digest and feed keep working.

## Threads

- A conversation is a thread. The title is the first 30 characters of your first question.
- A thread is created and saved when the first answer finishes, so no empty threads are left behind.
- The new-thread button starts a fresh conversation. The history button lists saved threads, opens one, or deletes one (with a confirmation).
- Threads are stored in the app database (tables `chat_threads` and `chat_messages`).

## Context chips

Under the header, the panel shows what the question is about, as chips:

- The scope you are viewing: all articles, a category, or a topic.
- The period: the digest period (today, this week, this month) or the feed period.
- Attached items: use the "ask in chat" button on a feed row or on a digest section to add that article or digest to the context. Each attached chip has a remove button.

The agent is told this context with every question. It reads an attached article with the `get_article` tool.

## Providers

| Provider | How it runs | Notes |
|---|---|---|
| Ollama | The agent calls the Ollama server at `ollama.base_url` with `ollama.model` (defaults `http://localhost:11434` and `gemma4:26b`). Context length is set to 16384. | Needs Ollama running and the model pulled. |
| Claude | The agent runs the `claude` CLI (`claude.bin`) with the model in `claude.model` (default `sonnet`). No session is kept on disk. | Needs the CLI signed in. The tools are passed to it as an MCP server. |

- The default provider comes from `llm.chat_provider` (default `ollama`). The selector in the panel's composer lists `Ollama · <model>` for every model installed in Ollama, then `Claude · sonnet`, `Claude · opus` and `Claude · fable` (plus `Claude · <name>` when `claude.model` holds a custom name). Choosing an entry switches the provider for that panel and writes `ollama.model` or `claude.model`, the same settings the LLM tab edits. It does not change `llm.chat_provider`. When the Ollama list cannot be fetched, only the current Ollama model is shown.
- There is no automatic fallback. If a provider fails, the error box offers a retry, a button to send the same question with the other provider, and a link to the LLM settings. Your question stays in the input.
- Error types are reported by kind: Ollama not reachable, model missing (with the `ollama pull` command), Claude not signed in, the `infodesk` CLI failing, or other.

## Tools

The agent reads your data through the `infodesk` CLI in JSON mode (see [CLI](08-cli.md)). It does not write to your data.

| Tool | What it does |
|---|---|
| `search_articles` | Search collected articles by keywords, category, topic, days, or a date range. Newest first. Up to 50 results (default 20). |
| `get_article` | Read one article: summary, article summary and URL. |
| `read_digests` | Read stored day, week or month digests. It does not generate them. Give a category or a topic, not both. |
| `fetch_page` | Fetch an article URL and return the body text. Not saved. Up to 20000 characters; with Ollama the default is 6000 to fit the context. |
| `update_plan` | Show a work plan of 1 to 8 steps. Used when a question needs two or more tool calls. |
| Web search | Search outside the collected articles. See below. |

### Web search

- Claude uses its built-in WebSearch.
- Ollama uses the ollama.com web search API. It is enabled only when you set an API key in Settings, LLM, "Web search API key" (`ollama.web_search_api_key`). Without a key the tool is not offered. Results are cut to 5 by default and 10 at most.
- Web search sends your query to an outside service. See [Privacy and Security](09-privacy-and-security.md).

## Progress display

Each answer shows its work while it runs and folds it into one line when it ends.

- A status line while running: "調査中 · 12秒 · ツール3回" (researching, elapsed seconds, tool count). It updates every second.
- A checklist of the work plan, when the model made one. When the answer finishes normally, every item shows as done.
- "考え中…" (thinking), a collapsible block with the model's reasoning, when the model returns reasoning.
- One row per tool call. Click a row to see the input and a summary of the result (for example the matched titles, or the page title and the first part of the body). Failed tools show the error.
- When an answer ends, the work folds into a line such as "4ステップ · 18秒". Steps count reasoning blocks and tool calls, not plan updates. Click the line to expand.
- An answer that did not finish writing (step limit, error or abort) shows "途中で停止" (stopped midway) and keeps the plan in its last state.
- Articles that the tools returned are listed as links under the answer (up to 8). Click one to open the article.

Limits per answer: 20 steps with Ollama, 20 turns with Claude.

## Links and images in answers

- Links in an answer open in the in-app reader window. Only http and https links are opened.
- Images in an answer are not loaded. Only the alt text is shown, so that showing an answer cannot make a request to an outside server.

## Prompt injection

The agent reads article text and web pages written by other people. That text can contain instructions aimed at the model. InfoDesk reduces the damage but cannot remove the risk:

- The tools are read-only. There is no tool that writes to your database, runs commands or reads local files.
- `fetch_page` refuses local and private addresses (see [Privacy and Security](09-privacy-and-security.md)).
- The system prompt tells the model to write only what it verified with tools.

Treat answers as a reading aid. Check the linked source before you act on a claim.

## Related pages

- [Settings](07-settings.md): `llm.chat_provider`, Ollama, Claude and Node paths, web search key.
- [Digests](04-digests.md): the periods the chat can read.
