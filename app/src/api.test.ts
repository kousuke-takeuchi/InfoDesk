import { describe, expect, it } from "vitest";
import { api } from "./api";
import { setupIpc } from "./test/ipc";
import type { ItemFilter } from "./types";

describe("api", () => {
  it("sends the full filter and camelCase arguments", async () => {
    const ipc = setupIpc({
      list_items: () => [],
      count_items: () => 7,
      get_digests: () => [],
      add_rss_source: (a) => ({ id: 1, kind: "rss", name: a.name ?? "t", category: a.category, config: a.url, enabled: true, last_fetched_at: null, last_error: null }),
    });
    const filter: ItemFilter = {
      category: "medcare", topic_id: null, source_id: null, since: null, until: null,
      text: null, unread_only: true, starred_only: false, order: "recent", limit: 50, offset: 0,
    };
    await api.listItems(filter);
    expect(ipc.last("list_items")).toEqual({ filter });
    expect(await api.countItems(filter)).toBe(7);
    expect(ipc.last("count_items")).toEqual({ filter });

    await api.getDigests("week", "2026-10-06", 3);
    expect(ipc.last("get_digests")).toEqual({ period: "week", date: "2026-10-06", topicId: 3 });

    const s = await api.addRssSource("https://e.com/feed", null, "tech_jp");
    expect(ipc.last("add_rss_source")).toEqual({ url: "https://e.com/feed", name: null, category: "tech_jp" });
    expect(s.config).toBe("https://e.com/feed");
  });

  it("rejects with the backend message", async () => {
    setupIpc({ collect_now: () => { throw { message: "collect already running" }; } });
    await expect(api.collectNow()).rejects.toMatchObject({ message: "collect already running" });
  });
});
