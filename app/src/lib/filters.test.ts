import { describe, expect, it } from "vitest";
import { buildItemFilter, type FeedState } from "./filters";

const base: FeedState = {
  selection: { kind: "all" }, range: "all", sourceId: null, unreadOnly: false, starredOnly: false, text: "",
};
const now = new Date(2026, 9, 6, 23, 30);

describe("buildItemFilter", () => {
  it("fills every field and maps the selection", () => {
    const f = buildItemFilter(base, now);
    expect(f).toEqual({
      category: null, topic_id: null, source_id: null, since: null, until: null, text: null,
      unread_only: false, starred_only: false, order: "recent", limit: 100, offset: 0,
    });
    expect(buildItemFilter({ ...base, selection: { kind: "category", category: "medcare" } }, now).category).toBe("medcare");
    expect(buildItemFilter({ ...base, selection: { kind: "topic", topicId: 4 } }, now).topic_id).toBe(4);
  });

  it("computes since from the local day start", () => {
    const today = buildItemFilter({ ...base, range: "today" }, now);
    expect(today.since).toBe(new Date(2026, 9, 6, 0, 0).toISOString());
    expect(today.until).toBeNull();
    const week = buildItemFilter({ ...base, range: "week" }, now);
    expect(week.since).toBe(new Date(2026, 9, 6 - 6, 0, 0).toISOString());
    expect(buildItemFilter({ ...base, range: "month" }, now).since).toBe(new Date(2026, 8, 7, 0, 0).toISOString());
    expect(buildItemFilter({ ...base, range: "3d" }, now).since).toBe(new Date(2026, 9, 4, 0, 0).toISOString());
  });

  it("trims text and passes flags", () => {
    const f = buildItemFilter({ ...base, text: "  認知症 ", unreadOnly: true, starredOnly: true, sourceId: 7 }, now, 30);
    expect(f.text).toBe("認知症");
    expect(f.unread_only).toBe(true);
    expect(f.starred_only).toBe(true);
    expect(f.source_id).toBe(7);
    expect(f.limit).toBe(30);
    expect(buildItemFilter({ ...base, text: "   " }, now).text).toBeNull();
  });
});
