import { describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { FeedView } from "./FeedView";
import { emitEvent, setupIpc } from "../test/ipc";
import type { AppStatus, Item } from "../types";

const status: AppStatus = { collecting: false, summarizing: false, digesting: false, last_collect_at: "2026-10-06T00:00:00Z", next_collect_at: null, last_error: null };
const item = (over: Partial<Item>): Item => ({
  id: 1, source_id: 1, source_name: "Google News: 認知症", category: "medcare", url: "https://e.com/1",
  title: "認知症の新薬", excerpt: "", author: null, published_at: null, fetched_at: new Date().toISOString(),
  score: null, ai_summary: "要約文。", read: false, starred: false, image_url: null, ...over,
});
const CHAT = { chatOpen: false, onToggleChat: () => {}, onAskChat: () => {}, onPeriodChange: () => {} };
const UNREAD_COUNT = /未読\s*\d+件/;
const tick = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });

function renderFeed(items: Item[], extra: Record<string, (a: any) => unknown> = {}, st: AppStatus = status, chat: Partial<typeof CHAT> = {}) {
  const ipc = setupIpc({
    list_items: () => items,
    count_items: (a) => items.filter((i) => !a.filter.unread_only || !i.read).length,
    list_sources: () => [{ id: 1, kind: "rss", name: "Google News: 認知症", category: "medcare", config: "u", enabled: true, last_fetched_at: null, last_error: null }],
    set_read: () => undefined, set_starred: () => undefined, open_item: () => undefined,
    mark_all_read: () => items.filter((i) => !i.read).length, collect_now: () => undefined,
    ...extra,
  });
  render(
    <FeedView selection={{ kind: "category", category: "medcare" }} topics={[]} status={st}
      view="feed" onSwitchView={() => {}} onOpenSources={() => {}} {...CHAT} {...chat} />,
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
    expect(ipc.unmocked).toEqual([]);
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

    setupIpc({ list_items: () => { throw { message: "database lock poisoned" }; }, count_items: () => 0, list_sources: () => [] });
    render(<FeedView selection={{ kind: "all" }} topics={[]} status={status} view="feed" onSwitchView={() => {}} onOpenSources={() => {}} {...CHAT} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("database lock poisoned");
  });

  it("shows the filtered empty state when items exist elsewhere", async () => {
    renderFeed([], {
      list_items: (a) => (a.filter.unread_only ? [] : [item({ read: true })]),
      count_items: (a) => (a.filter.unread_only ? 0 : 1),
    });
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

  it("shows the backend unread total instead of the page count", async () => {
    const ipc = renderFeed([item({}), item({ id: 2, title: "二本目" })], {
      count_items: (a) => (a.filter.unread_only ? 137 : 250),
    });
    expect(await screen.findByText("未読 137件")).toBeInTheDocument();
    expect(screen.queryByText("未読 2件")).not.toBeInTheDocument();
    // 件数の問い合わせは一覧と同じ絞り込みで、未読の合計だけ unread_only を立てる
    const page = ipc.last("list_items").filter;
    const counts = ipc.calls.filter((c) => c.cmd === "count_items").map((c) => c.args.filter);
    expect(counts).toContainEqual(page);
    expect(counts).toContainEqual({ ...page, unread_only: true });
  });

  it("renders no unread count before the first response", async () => {
    let resolve: (v: Item[]) => void = () => {};
    renderFeed([], {
      list_items: () => new Promise<Item[]>((r) => { resolve = r; }),
      count_items: (a) => (a.filter.unread_only ? 137 : 250),
    });
    await tick();
    expect(screen.queryByText(UNREAD_COUNT)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "すべて既読にする" })).not.toBeInTheDocument();
    await act(async () => { resolve([item({})]); });
    expect(await screen.findByText("未読 137件")).toBeInTheDocument();
  });

  it("renders no unread count after a failed load", async () => {
    let fail = false;
    const ipc = renderFeed([item({})], {
      list_items: () => { if (fail) throw { message: "database lock poisoned" }; return [item({})]; },
      count_items: () => 5,
    });
    expect(await screen.findByText("未読 5件")).toBeInTheDocument();
    fail = true;
    await emitEvent("items-changed");
    expect(await screen.findByRole("alert")).toHaveTextContent("database lock poisoned");
    expect(screen.queryByText(UNREAD_COUNT)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "すべて既読にする" })).not.toBeInTheDocument();
    expect(ipc.unmocked).toEqual([]);
  });

  it("hides mark-all-read when the backend reports no unread items", async () => {
    renderFeed([item({ read: true })]);
    expect(await screen.findByText("未読 0件")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "すべて既読にする" })).not.toBeInTheDocument();
  });

  it("shows the total, loads more, and resets the limit when a filter changes", async () => {
    const ipc = renderFeed([item({}), item({ id: 2, title: "二本目" })], {
      count_items: (a) => (a.filter.unread_only ? 137 : 250),
    });
    expect(await screen.findByText("250件中 2件を表示")).toBeInTheDocument();
    expect(ipc.last("list_items").filter.limit).toBe(100);

    await userEvent.click(screen.getByRole("button", { name: "さらに表示" }));
    await waitFor(() => expect(ipc.last("list_items").filter.limit).toBe(200));

    await userEvent.click(screen.getByRole("button", { name: "未読のみ" }));
    await waitFor(() => expect(ipc.last("list_items").filter.unread_only).toBe(true));
    await tick();
    const unreadCalls = ipc.calls.filter((c) => c.cmd === "list_items" && c.args.filter.unread_only);
    expect(unreadCalls.map((c) => c.args.filter.limit)).toEqual([100]);
  });

  it("shows no footer when everything is displayed", async () => {
    renderFeed([item({})]);
    await screen.findByText("未読 1件");
    expect(screen.queryByText(/件を表示/)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "さらに表示" })).not.toBeInTheDocument();
  });

  it("debounces the search text into a single request", async () => {
    const ipc = renderFeed([item({})]);
    await screen.findByText("未読 1件");
    const before = ipc.count("list_items");
    const input = screen.getByRole("textbox", { name: "検索" });
    await userEvent.type(input, "新薬");
    expect(input).toHaveValue("新薬");
    await waitFor(() => expect(ipc.last("list_items").filter.text).toBe("新薬"));
    await tick();
    const texts = ipc.calls.filter((c) => c.cmd === "list_items").map((c) => c.args.filter.text);
    expect(texts).not.toContain("新");
    expect(ipc.count("list_items")).toBe(before + 1);
  });

  it("ignores intermediate text while an IME composition is in progress", async () => {
    const ipc = renderFeed([item({})]);
    await screen.findByText("未読 1件");
    const input = screen.getByRole("textbox", { name: "検索" });
    fireEvent.compositionStart(input);
    fireEvent.change(input, { target: { value: "しんやく" } });
    expect(input).toHaveValue("しんやく");
    // デバウンスより長く待っても、変換中の文字列では問い合わせない
    await act(async () => { await new Promise((r) => setTimeout(r, 350)); });
    fireEvent.change(input, { target: { value: "新薬" } });
    fireEvent.compositionEnd(input);
    await waitFor(() => expect(ipc.last("list_items").filter.text).toBe("新薬"));
    const texts = ipc.calls.filter((c) => c.cmd === "list_items").map((c) => c.args.filter.text);
    expect(texts).not.toContain("しんやく");
  });

  it("drops a stale response that arrives after a newer one", async () => {
    let resolveSlow: (v: Item[]) => void = () => {};
    const ipc = renderFeed([], {
      list_items: (a) => {
        if (a.filter.starred_only) return [item({ id: 3, title: "新しい結果" })];
        if (a.filter.unread_only) return new Promise<Item[]>((r) => { resolveSlow = r; });
        return [item({})];
      },
      count_items: () => 1,
    });
    await screen.findByText("認知症の新薬");
    await userEvent.click(screen.getByRole("button", { name: "未読のみ" }));
    await waitFor(() => expect(ipc.last("list_items").filter.unread_only).toBe(true));
    await userEvent.click(screen.getByRole("button", { name: /スター$/ }));
    expect(await screen.findByText("新しい結果")).toBeInTheDocument();
    await act(async () => { resolveSlow([item({ id: 4, title: "古い結果" })]); });
    await tick();
    expect(screen.getByText("新しい結果")).toBeInTheDocument();
    expect(screen.queryByText("古い結果")).not.toBeInTheDocument();
  });

  it("keeps an action error across a reload and clears it on the next successful action", async () => {
    const ipc = renderFeed([item({})], {
      open_item: () => { throw { message: "unsupported URL scheme" }; },
    });
    await screen.findByText("認知症の新薬");
    await userEvent.click(screen.getByRole("button", { name: "認知症の新薬" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("unsupported URL scheme");

    const before = ipc.count("list_items");
    await emitEvent("items-changed");
    await waitFor(() => expect(ipc.count("list_items")).toBeGreaterThan(before));
    await tick();
    expect(screen.getByRole("alert")).toHaveTextContent("unsupported URL scheme");

    await userEvent.click(screen.getByRole("button", { name: "スターを付ける" }));
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
  });

  it("does not reload by itself after an action that emits items-changed", async () => {
    const ipc = renderFeed([item({})]);
    await screen.findByText("未読 1件");
    const before = ipc.count("list_items");
    await userEvent.click(screen.getByRole("button", { name: "スターを付ける" }));
    await userEvent.click(screen.getByRole("button", { name: "すべて既読にする" }));
    await tick();
    expect(ipc.count("list_items")).toBe(before);
  });
  it("offers ソースを確認 next to 今すぐ収集 in the first-launch empty state", async () => {
    let opened = 0;
    setupIpc({ list_items: () => [], count_items: () => 0, list_sources: () => [], collect_now: () => undefined });
    render(<FeedView selection={{ kind: "all" }} topics={[]} status={{ ...status, last_collect_at: null }}
      view="feed" onSwitchView={() => {}} onOpenSources={() => { opened += 1; }} {...CHAT} />);
    expect(await screen.findByText("まだ記事がありません")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "今すぐ収集" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "ソースを確認" }));
    expect(opened).toBe(1);
  });

  it("clears the previous selection's rows as soon as the selection changes", async () => {
    let resolveSlow: (v: Item[]) => void = () => {};
    const ipc = setupIpc({
      list_items: (a) => (a.filter.category === "medcare"
        ? [item({})]
        : new Promise<Item[]>((r) => { resolveSlow = r; })),
      count_items: () => 1,
      list_sources: () => [],
    });
    const props = { topics: [], status, view: "feed" as const, onSwitchView: () => {}, onOpenSources: () => {}, ...CHAT };
    const { rerender } = render(<FeedView selection={{ kind: "category", category: "medcare" }} {...props} />);
    expect(await screen.findByText("認知症の新薬")).toBeInTheDocument();
    expect(screen.getByText(UNREAD_COUNT)).toBeInTheDocument();

    rerender(<FeedView selection={{ kind: "all" }} {...props} />);
    expect(screen.getByRole("heading", { name: "すべての記事" })).toBeInTheDocument();
    expect(screen.queryByText("認知症の新薬")).not.toBeInTheDocument();
    expect(screen.queryByText(UNREAD_COUNT)).not.toBeInTheDocument();
    expect(screen.queryByText("条件に一致する記事がありません")).not.toBeInTheDocument();
    await waitFor(() => expect(ipc.last("list_items").filter.category).toBeNull());

    await act(async () => { resolveSlow([item({ id: 7, title: "全体の記事" })]); });
    expect(await screen.findByText("全体の記事")).toBeInTheDocument();
  });

  it("keeps the rows while a filter changes within the same selection", async () => {
    let resolveSlow: (v: Item[]) => void = () => {};
    const ipc = renderFeed([], {
      list_items: (a) => (a.filter.unread_only ? new Promise<Item[]>((r) => { resolveSlow = r; }) : [item({})]),
      count_items: () => 1,
    });
    await screen.findByText("認知症の新薬");
    await userEvent.click(screen.getByRole("button", { name: "未読のみ" }));
    await waitFor(() => expect(ipc.last("list_items").filter.unread_only).toBe(true));
    expect(screen.getByText("認知症の新薬")).toBeInTheDocument();
    await act(async () => { resolveSlow([item({ id: 8, title: "未読の記事" })]); });
    expect(await screen.findByText("未読の記事")).toBeInTheDocument();
  });

  it("has a chat toggle that reflects and flips the panel state", async () => {
    const onToggleChat = vi.fn();
    renderFeed([item({})], {}, status, { onToggleChat });
    await screen.findByText("認知症の新薬");
    const open = screen.getByRole("button", { name: "チャットを開く" });
    expect(open).toHaveAttribute("aria-pressed", "false");
    await userEvent.click(open);
    expect(onToggleChat).toHaveBeenCalledTimes(1);
  });

  it("shows the toggle as pressed while the chat is open", async () => {
    renderFeed([item({})], {}, status, { chatOpen: true });
    await screen.findByText("認知症の新薬");
    expect(screen.getByRole("button", { name: "チャットを閉じる" })).toHaveAttribute("aria-pressed", "true");
  });

  it("asks about an item in chat without opening it", async () => {
    const onAskChat = vi.fn();
    const ipc = renderFeed([item({})], {}, status, { onAskChat });
    await screen.findByText("認知症の新薬");
    await userEvent.click(screen.getByRole("button", { name: "認知症の新薬 についてチャットで質問" }));
    expect(onAskChat).toHaveBeenCalledWith({ type: "item", id: 1, title: "認知症の新薬" });
    expect(ipc.count("open_item")).toBe(0);
  });

  it("reports the period on mount and when it changes", async () => {
    const onPeriodChange = vi.fn();
    renderFeed([item({})], {}, status, { onPeriodChange });
    await screen.findByText("認知症の新薬");
    expect(onPeriodChange).toHaveBeenCalledTimes(1);
    expect(onPeriodChange).toHaveBeenLastCalledWith({ view: "feed", range: "week" });
    await userEvent.click(screen.getByRole("button", { name: "3日" }));
    expect(onPeriodChange).toHaveBeenLastCalledWith({ view: "feed", range: "3d" });
    expect(onPeriodChange).toHaveBeenCalledTimes(2);
  });

  it("shows a thumbnail for articles with an https image and hides it when it fails to load", async () => {
    renderFeed([
      item({ image_url: "https://img.example.com/a.jpg" }),
      item({ id: 2, title: "画像なし", image_url: null }),
    ]);
    await screen.findByText("認知症の新薬");
    const img = document.querySelector("img.thumb") as HTMLImageElement;
    expect(img).not.toBeNull();
    expect(img).toHaveAttribute("src", "https://img.example.com/a.jpg");
    expect(img).toHaveAttribute("loading", "lazy");
    expect(img).toHaveAttribute("referrerpolicy", "no-referrer");
    expect(img).toHaveAttribute("alt", "");
    expect(document.querySelectorAll(".item-row img")).toHaveLength(1);
    fireEvent.error(img);
    expect(document.querySelector(".item-row img")).toBeNull();
  });

  it("does not render non-https image URLs", async () => {
    renderFeed([item({ image_url: "http://img.example.com/a.jpg" })]);
    await screen.findByText("認知症の新薬");
    expect(document.querySelector(".item-row img")).toBeNull();
  });

  it("opens the article when the thumbnail is clicked", async () => {
    const ipc = renderFeed([item({ image_url: "https://img.example.com/a.jpg" })]);
    await screen.findByText("認知症の新薬");
    await userEvent.click(document.querySelector("img.thumb") as HTMLImageElement);
    expect(ipc.last("open_item")).toEqual({ id: 1 });
  });

  it("places the thumbnail between the content and the row actions", async () => {
    renderFeed([item({ image_url: "https://img.example.com/a.jpg" })]);
    await screen.findByText("認知症の新薬");
    const row = document.querySelector(".item-row") as HTMLElement;
    const kids = Array.from(row.children);
    const idx = (el: Element | null) => kids.indexOf(el as Element);
    const thumb = idx(row.querySelector("img.thumb"));
    expect(thumb).toBeGreaterThan(idx(row.querySelector(".content")));
    expect(thumb).toBeLessThan(idx(row.querySelector(".row-actions")));
    expect(thumb).toBeLessThan(idx(row.querySelector(".star")));
    expect(row.querySelector("img.thumb")).not.toHaveAttribute("tabindex");
  });
});
