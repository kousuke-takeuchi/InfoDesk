import { describe, expect, it } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { FeedView } from "./FeedView";
import { emitEvent, setupIpc } from "../test/ipc";
import type { AppStatus, Item } from "../types";

const status: AppStatus = { collecting: false, summarizing: false, digesting: false, last_collect_at: "2026-10-06T00:00:00Z", next_collect_at: null, last_error: null };
const item = (over: Partial<Item>): Item => ({
  id: 1, source_id: 1, source_name: "Google News: 認知症", category: "medcare", url: "https://e.com/1",
  title: "認知症の新薬", excerpt: "", author: null, published_at: null, fetched_at: new Date().toISOString(),
  score: null, ai_summary: "要約文。", read: false, starred: false, ...over,
});

function renderFeed(items: Item[], extra: Record<string, (a: any) => unknown> = {}, st: AppStatus = status) {
  const ipc = setupIpc({
    list_items: () => items,
    list_sources: () => [{ id: 1, kind: "rss", name: "Google News: 認知症", category: "medcare", config: "u", enabled: true, last_fetched_at: null, last_error: null }],
    set_read: () => undefined, set_starred: () => undefined, open_item: () => undefined,
    mark_all_read: () => items.filter((i) => !i.read).length, collect_now: () => undefined,
    ...extra,
  });
  render(
    <FeedView selection={{ kind: "category", category: "medcare" }} topics={[]} status={st}
      view="feed" onSwitchView={() => {}} />,
  );
  return ipc;
}

describe("FeedView", () => {
  it("renders items with unread, summary, and sends the full filter", async () => {
    const ipc = renderFeed([item({}), item({ id: 2, title: "既読の記事", read: true, starred: true, score: 42, ai_summary: null })]);
    expect(await screen.findByText("認知症の新薬")).toBeInTheDocument();
    expect(screen.getByText("要約文。")).toBeInTheDocument();
    expect(screen.getByText("既読の記事").closest(".item-row")).toHaveClass("read");
    expect(screen.getByText("42")).toBeInTheDocument();
    const filter = ipc.last("list_items").filter;
    expect(filter.category).toBe("medcare");
    expect(filter).toMatchObject({ unread_only: false, starred_only: false, order: "recent", offset: 0 });
    expect(Object.keys(filter).sort()).toEqual(
      ["category", "limit", "offset", "order", "since", "source_id", "starred_only", "text", "topic_id", "unread_only", "until"],
    );
    expect(screen.getByRole("heading", { name: /医療介護/ })).toBeInTheDocument();
    expect(screen.getByText("未読 1件")).toBeInTheDocument();
  });

  it("opens, stars, filters, and marks all read", async () => {
    const ipc = renderFeed([item({})]);
    await screen.findByText("認知症の新薬");
    await userEvent.click(screen.getByRole("button", { name: "認知症の新薬" }));
    expect(ipc.last("open_item")).toEqual({ id: 1 });

    await userEvent.click(screen.getByRole("button", { name: "スターを付ける" }));
    expect(ipc.last("set_starred")).toEqual({ id: 1, starred: true });

    await userEvent.click(screen.getByRole("button", { name: "未読のみ" }));
    await waitFor(() => expect(ipc.last("list_items").filter.unread_only).toBe(true));

    await userEvent.type(screen.getByPlaceholderText("検索"), "新薬");
    await waitFor(() => expect(ipc.last("list_items").filter.text).toBe("新薬"));

    await userEvent.click(screen.getByRole("button", { name: "すべて既読にする" }));
    expect(ipc.last("mark_all_read").filter.category).toBe("medcare");
  });

  it("shows the first-launch empty state and the error line", async () => {
    // Review Focus 1 と 2
    const ipc = renderFeed([], {}, { ...status, last_collect_at: null });
    expect(await screen.findByText("まだ記事がありません")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "今すぐ収集" }));
    expect(ipc.count("collect_now")).toBe(1);

    setupIpc({ list_items: () => { throw { message: "database lock poisoned" }; }, list_sources: () => [] });
    render(<FeedView selection={{ kind: "all" }} topics={[]} status={status} view="feed" onSwitchView={() => {}} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("database lock poisoned");
  });

  it("shows the filtered empty state when items exist elsewhere", async () => {
    renderFeed([], { list_items: (a) => (a.filter.unread_only ? [] : [item({ read: true })]) });
    await screen.findByText("認知症の新薬");
    await userEvent.click(screen.getByRole("button", { name: "未読のみ" }));
    expect(await screen.findByText("条件に一致する記事がありません")).toBeInTheDocument();
  });

  it("refetches the list when items-changed is emitted", async () => {
    const ipc = renderFeed([item({})]);
    await screen.findByText("認知症の新薬");
    const before = ipc.count("list_items");
    await emitEvent("items-changed");
    await waitFor(() => expect(ipc.count("list_items")).toBeGreaterThan(before));
  });
});
