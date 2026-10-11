**日本語: [01-overview.ja.md](01-overview.ja.md)** | Next: [Getting Started →](02-getting-started.md)

# Overview

## What is InfoDesk?

InfoDesk is a macOS desktop app that collects tech and healthcare/nursing-care news, then writes **daily, weekly and monthly digests in Japanese**. It runs on your Mac. Articles are stored in a local SQLite file, and summaries can be produced by a local LLM (Ollama) so that nothing has to leave the machine.

It is an alpha-stage personal project (version 0.1.0). macOS is the only supported platform, and Apple Silicon is the only one tested.

## Who is it for?

- People who follow several English and Japanese sources and want one short Japanese briefing per day, week or month
- People who work in healthcare or nursing care and also follow tech news
- People who prefer a local LLM, or who already use the `claude` CLI, over a hosted news service

## What it does

| Step | What happens | Page |
|---|---|---|
| Collect | Fetches GitHub Trending, arXiv, Hugging Face, Hacker News and RSS feeds on a schedule | [Sources and Topics](03-sources-and-topics.md) |
| Summarize | Writes a Japanese one or two sentence summary for the top articles of each category (English articles are translated at the same time) | [Digests](04-digests.md) |
| Digest | Builds day, week and month digests per category and per topic, with an overview of counts and trends | [Digests](04-digests.md) |
| Feed | Lists articles with filters, thumbnails, read and star marks | [Feed and Reader](05-feed-and-reader.md) |
| Reader | Opens articles in an in-app reader window | [Feed and Reader](05-feed-and-reader.md) |
| Chat | Lets you ask an agent about the collected articles and digests | [AI Chat](06-ai-chat.md) |

The same data is available from the `infodesk` command line tool.

## Design goals

- **Local-first**: articles, summaries, settings and chat threads live in one SQLite file under `~/Library/Application Support/`. There is no InfoDesk server and no account.
- **Local LLM allowed**: the default provider is Ollama. You can switch article summaries, period digests and chat to the `claude` CLI separately.
- **Japanese output**: every summary and digest is written in Japanese, whatever language the source uses.
- **Plain and inspectable**: the interface follows the macOS appearance (light and dark), and each digest links back to the articles it is based on.

## Current status

Alpha. The collector, summaries, digests, feed, in-app reader and chat all work. Known limits and ideas are listed in the [roadmap](10-roadmap.md). To try it, go to [Getting Started](02-getting-started.md).
