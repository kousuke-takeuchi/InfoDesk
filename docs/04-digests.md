# Digests

**日本語: [04-digests.ja.md](04-digests.ja.md)** | ← [Sources and Topics](03-sources-and-topics.md) | Next: [Feed and Reader →](05-feed-and-reader.md)

A digest is a Japanese summary of one period. Open it with the **Digest** switch in the toolbar.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="images/digest-week-dark.png">
  <img src="images/digest-week-light.png" alt="The weekly digest: overview charts at the top, then one section per category with a headline, numbered points and image cards (sample data)" width="800">
</picture>

*The weekly digest in light and dark appearance (sample data).*

## 1. Periods

Use the segmented control in the toolbar to choose **Today**, **Week** or **Month**. The screen always shows the current period, and the heading shows its dates.

| Period | Starts | Ends (exclusive) |
|---|---|---|
| Day | local midnight | the next local midnight |
| Week | Monday, local midnight | the next Monday |
| Month | the 1st, local midnight | the 1st of the next month |

Boundaries use your local time zone. A period is a half-open range: it includes its start and excludes its end.

## 2. How a digest is built

Digests are built in layers, so a long period reads from shorter ones.

- **Day**: for each category, the model reads the titles and article summaries of that day's articles.
- **Week**: for each category, the model reads the daily digests of that week.
- **Month**: for each category, the model reads the weekly digests of the weeks that overlap the month, from the week containing the 1st to the week containing the last day. A week that spans two months feeds both.
- A category with no articles in the period gets no digest.
- Each digest keeps the list of articles it is based on, and shows the provider that wrote it (for example `ollama:gemma4:26b`) and the article count.

The provider for article summaries and for period digests are separate settings (`llm.items_provider`, `llm.digest_provider`), so you can use Ollama for the many article summaries and Claude for the digests.

When digests are generated is described in [Sources and Topics](03-sources-and-topics.md#5-collection-schedule).

## 3. Category and topic digests

The sidebar selection decides what you see.

- **All** or a **category**: one section per category (or the one category you chose).
- A **topic**: one section for that topic. It is built from the titles and summaries of the matching articles, and is generated on demand: press **Generate** for that topic and period. The result is saved and reused.

Each section has a button to ask the chat about it.

## 4. Period overview

The top of the screen shows an overview computed from the database only. It does not call an LLM. It counts articles by the time they were fetched, inside the same period boundaries as above.

- **Counts by category**: horizontal bars. For week and month, each bar has a small line chart of daily counts.
- **Topic strength**: for each topic, the article count and the change from the previous period of the same length (yesterday, last week, last month). An up arrow means more, a down arrow means fewer, and a dash means no change. When one topic is selected, only that topic is shown.

The charts are drawn without color alone: the numbers are printed next to the bars.

## 5. Reading a section

For a digest that has finished:

- **Headline**: the first line of the digest, shown large. The model is asked for one line of at most 40 characters. Digests created before this was added have no headline and show as plain text.
- **Numbered points**: the body, as a numbered list.
- **Image cards**: from the articles the digest is based on, the first three that have an https image are shown as cards (image, title, source). Press a card to open the article in the [reader](05-feed-and-reader.md#2-reader). If an image fails to load, the card falls back to a text link.
- **References**: the remaining articles appear as text links, with a count of the rest.

## 6. Regenerate, pending and failed

Press **Regenerate** in the heading to rebuild the visible sections. Each section has its own state, and one failure does not affect the others.

| State | Meaning | What you can do |
|---|---|---|
| Generating | The model is writing the digest | Wait |
| Pending | A generation failed and will be retried at the next scheduled run. The last error is shown | **Retry now** |
| Failed | Generation failed 3 times. The number of attempts and the last error are shown | Check the LLM settings, then **Retry** |
| Not generated yet | There are articles but no digest | **Generate** |
| No articles | Nothing was collected in this period | None |

A failed attempt goes back to pending until the third failure, after which it is marked failed. At the end of each scheduled run, pending digests are retried automatically. A successful run recovers the digest.
