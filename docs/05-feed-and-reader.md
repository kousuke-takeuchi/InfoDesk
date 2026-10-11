# Feed and Reader

**日本語: [05-feed-and-reader.ja.md](05-feed-and-reader.ja.md)** | ← [Digests](04-digests.md) | Next: [AI Chat →](06-ai-chat.md)

## 1. The feed

The **Feed** switch in the toolbar lists collected articles, newest first.

<picture>
  <img src="images/feed.png" alt="The feed: filter toolbar, article rows with unread dots, source, time, summary, thumbnail and star (sample data)" width="800">
</picture>

*The feed (sample data).*

Each row shows:

- a blue dot when the article is unread (the title is also bold)
- the title, the source name, a relative time and a score when the source has one (stars, points)
- the Japanese article summary, if the article was among the top ones summarized
- a thumbnail on the right, if the article has an image (see [Sources and Topics](03-sources-and-topics.md#4-article-images))
- a chat button, which starts a question about that article
- a star button

The heading shows the unread count, and a **Mark all read** link. The link applies to the current filters, not to the whole database.

### Filters

The sidebar selection (All, one category, or one topic) sets the scope. The toolbar adds:

| Filter | Options |
|---|---|
| Period | Today, 3 days, 1 week, 1 month, All. These count back from the start of today, not calendar weeks |
| Source | All sources, or one source |
| Unread only | Toggle |
| Starred | Toggle |
| Search | Free text, matched against the title and excerpt |

The list loads 100 articles at a time. When there are more, press **Show more**. The footer shows how many of the total are displayed.

If nothing has been collected yet, the feed shows an empty state with **Collect now** and **Check sources** buttons.

### Read and star

- Opening an article marks it read.
- The star button toggles a star. Use the **Starred** filter to list them.
- Counts in the sidebar update when you read, star or collect.

## 2. Reader

Articles and links open in an in-app reader window instead of your default browser. This applies wherever you open an article: the feed (title or thumbnail), digest references and image cards, and links in chat answers.

The reader is one window, reused: opening another article navigates the same window and brings it to the front. The window has a toolbar at the top and the article page below it.

| Toolbar control | Action |
|---|---|
| Back, Forward | Move in the page history |
| Reload | Reload the page |
| Page title and URL | Shows the current page |
| Open in browser | Hands the current page to your default browser |
| Close | Closes the reader window |

Rules for what opens where:

- Only `http` and `https` addresses open. Navigation inside the reader to any other scheme is blocked.
- Links that ask for a new window (`target="_blank"`) open in the same reader window.
- Addresses that point at your own machine or local network (localhost, loopback, private and link-local ranges) are not loaded in the reader. They open in your default browser instead.
- Article pages get no access to the app's internal commands. See the privacy page for the isolation details.

To ask the chat about an article, press the chat button on its row. See [AI Chat](06-ai-chat.md).
