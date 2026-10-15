import { describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import App from "./App";
import { EVENTS } from "./api";
import { emitEvent, setupIpc, type IpcHandlers } from "./test/ipc";
import type { AppStatus, SidebarCounts, Topic } from "./types";

// App が useStaleRefresh に渡した関数を取り出せるようにする (実物もそのまま動く)。
const stale = vi.hoisted(() => ({ fn: null as null | (() => void) }));
vi.mock("./hooks/useStaleRefresh", async (orig) => {
  const real = await orig<typeof import("./hooks/useStaleRefresh")>();
  return { useStaleRefresh: (cb: () => void) => { stale.fn = cb; real.useStaleRefresh(cb); } };
});

const STATUS: AppStatus = {
  collecting: false, summarizing: false, digesting: false,
  last_collect_at: null, next_collect_at: null, last_error: null,
};
const TOPICS: Topic[] = [];

function counts(total: number): SidebarCounts {
  return { total_unread: total, categories: {}, topics: {} };
}

function handlers(over: IpcHandlers = {}): IpcHandlers {
  return {
    get_status: () => STATUS,
    sidebar_counts: () => counts(7),
    list_topics: () => TOPICS,
    collect_now: () => undefined,
    get_digests: () => [],
    digest_stats: () => ({ period: "day", period_start: "2026-10-06", total: 0, categories: [], topics: [] }),
    get_item: () => null,
    chat_endpoint: () => ({ status: "starting" }),
    list_chat_threads: () => [],
    get_settings: () => ({}),
    ...over,
  };
}

const allItem = () => screen.getByRole("button", { name: /すべて/ });

describe("App", () => {
  it("loads status, counts, and topics on mount and renders the counts", async () => {
    const ipc = setupIpc(handlers());
    render(<App />);
    await waitFor(() => expect(allItem()).toHaveTextContent("7"));
    expect(ipc.count("get_status")).toBe(1);
    expect(ipc.count("sidebar_counts")).toBe(1);
    expect(ipc.count("list_topics")).toBe(1);
    expect(ipc.unmocked).toEqual([]);
  });

  it("refetches counts when items-changed fires", async () => {
    let total = 7;
    const ipc = setupIpc(handlers({ sidebar_counts: () => counts(total) }));
    render(<App />);
    await waitFor(() => expect(allItem()).toHaveTextContent("7"));

    total = 12;
    await emitEvent(EVENTS.items);
    await waitFor(() => expect(allItem()).toHaveTextContent("12"));
    expect(ipc.count("sidebar_counts")).toBe(2);
  });

  it("refetches status when status-changed fires", async () => {
    let status = STATUS;
    const ipc = setupIpc(handlers({ get_status: () => status }));
    render(<App />);
    await waitFor(() => expect(ipc.count("get_status")).toBe(1));
    expect(await screen.findByText("未収集")).toBeInTheDocument();

    status = { ...STATUS, collecting: true };
    await emitEvent(EVENTS.status);
    expect(await screen.findByText("収集中…")).toBeInTheDocument();
    expect(ipc.count("get_status")).toBe(2);
  });

  it("switches to settings when open-settings fires", async () => {
    setupIpc(handlers({ list_sources: () => [], source_item_counts: () => ({}) }));
    render(<App />);
    await waitFor(() => expect(allItem()).toHaveTextContent("7"));
    const settings = screen.getByRole("button", { name: "設定" });
    expect(settings).not.toHaveAttribute("aria-current");

    await emitEvent(EVENTS.openSettings);
    await waitFor(() => expect(settings).toHaveAttribute("aria-current", "true"));
  });

  it("keeps a get_status error when counts later succeed", async () => {
    const ipc = setupIpc(handlers({
      get_status: () => { throw { kind: "internal", message: "status が取得できません" }; },
    }));
    render(<App />);
    expect(await screen.findByRole("alert")).toHaveTextContent("status が取得できません");
    await waitFor(() => expect(allItem()).toHaveTextContent("7"));

    await emitEvent(EVENTS.items);
    await waitFor(() => expect(ipc.count("sidebar_counts")).toBe(2));
    await waitFor(() => expect(ipc.count("list_topics")).toBe(2));
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByRole("alert")).toHaveTextContent("status が取得できません");
  });

  it("shows a collect_now failure", async () => {
    setupIpc(handlers({
      collect_now: () => { throw { kind: "internal", message: "収集を開始できません" }; },
    }));
    render(<App />);
    await waitFor(() => expect(allItem()).toHaveTextContent("7"));
    expect(screen.queryByRole("alert")).toBeNull();

    await userEvent.click(screen.getByTitle("今すぐ収集 (⌘R)"));
    expect(await screen.findByRole("alert")).toHaveTextContent("収集を開始できません");
  });
  it("opens the sources settings from the first-launch empty feed", async () => {
    setupIpc(handlers({
      list_items: () => [], count_items: () => 0,
      list_sources: () => [], source_item_counts: () => ({}),
    }));
    render(<App />);
    await waitFor(() => expect(allItem()).toHaveTextContent("7"));
    await userEvent.click(screen.getByRole("button", { name: "フィード" }));
    await userEvent.click(await screen.findByRole("button", { name: "ソースを確認" }));
    expect(screen.getByRole("button", { name: "設定" })).toHaveAttribute("aria-current", "true");
    expect(await screen.findByRole("heading", { name: "ソース" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "RSSを追加" })).toBeInTheDocument();
  });

  it("resets the selection to all when the selected topic disappears", async () => {
    let topics: Topic[] = [{ id: 5, name: "認知症", keywords: ["認知症"] }];
    const ipc = setupIpc(handlers({ list_topics: () => topics }));
    render(<App />);
    const topic = await screen.findByRole("button", { name: /認知症/ });
    await userEvent.click(topic);
    expect(topic).toHaveAttribute("aria-current", "true");
    await waitFor(() => expect(ipc.last("get_digests").topicId).toBe(5));

    topics = [];
    await emitEvent(EVENTS.digests);
    await waitFor(() => expect(screen.queryByRole("button", { name: /認知症/ })).not.toBeInTheDocument());
    expect(allItem()).toHaveAttribute("aria-current", "true");
    await waitFor(() => expect(ipc.last("get_digests").topicId).toBeNull());
  });

  it("keeps a topic selection that is still in the refreshed list", async () => {
    const topics: Topic[] = [{ id: 5, name: "認知症", keywords: ["認知症"] }];
    const ipc = setupIpc(handlers({ list_topics: () => topics }));
    render(<App />);
    const topic = await screen.findByRole("button", { name: /認知症/ });
    await userEvent.click(topic);
    await emitEvent(EVENTS.items);
    await waitFor(() => expect(ipc.count("list_topics")).toBe(2));
    await new Promise((r) => setTimeout(r, 0));
    expect(topic).toHaveAttribute("aria-current", "true");
  });

  it("returns to the view shown before settings when a sidebar item is picked", async () => {
    setupIpc(handlers({
      list_items: () => [], count_items: () => 0,
      list_sources: () => [], source_item_counts: () => ({}),
    }));
    render(<App />);
    await waitFor(() => expect(allItem()).toHaveTextContent("7"));
    const feed = () => screen.getByRole("button", { name: "フィード" });

    await userEvent.click(feed());
    expect(feed()).toHaveAttribute("aria-pressed", "true");
    await userEvent.click(screen.getByRole("button", { name: "設定" }));
    expect(await screen.findByRole("button", { name: "RSSを追加" })).toBeInTheDocument();
    // 設定を開いたまま再度開いても、戻り先は変わらない
    await emitEvent(EVENTS.openSettings);
    await userEvent.click(screen.getByRole("button", { name: "GitHub" }));
    expect(feed()).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("heading", { name: "GitHub" })).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "ダイジェスト" }));
    await userEvent.click(screen.getByRole("button", { name: "設定" }));
    await userEvent.click(allItem());
    expect(screen.getByRole("button", { name: "ダイジェスト" })).toHaveAttribute("aria-pressed", "true");
  });

  describe("chat panel", () => {
    const ITEM = {
      id: 12, source_id: 1, source_name: "s", category: "medcare", url: "https://e.com/12", title: "認知症の新薬",
      excerpt: "", author: null, published_at: null, fetched_at: new Date().toISOString(),
      score: null, ai_summary: null, read: false, starred: false, image_url: null,
    };
    const feedHandlers = (over: IpcHandlers = {}) => handlers({
      list_items: () => [ITEM], count_items: () => 1, list_sources: () => [], source_item_counts: () => ({}), ...over,
    });
    const panel = () => screen.queryByRole("complementary", { name: "チャット" });

    it("opens and closes with the toolbar toggle and the close button", async () => {
      setupIpc(handlers());
      render(<App />);
      await waitFor(() => expect(allItem()).toHaveTextContent("7"));
      expect(panel()).toBeNull();
      await userEvent.click(screen.getByRole("button", { name: "チャットを開く" }));
      expect(panel()).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "チャットを閉じる", pressed: true })).toBeInTheDocument();
      await userEvent.click(within(panel()!).getByRole("button", { name: "チャットを閉じる" }));
      expect(panel()).toBeNull();
    });

    it("opens the panel with an item chip when asking about an item, once per item", async () => {
      setupIpc(feedHandlers());
      render(<App />);
      await waitFor(() => expect(allItem()).toHaveTextContent("7"));
      await userEvent.click(screen.getByRole("button", { name: "フィード" }));
      const ask = await screen.findByRole("button", { name: "認知症の新薬 についてチャットで質問" });
      await userEvent.click(ask);
      const chips = within(await screen.findByRole("group", { name: "文脈" }));
      expect(chips.getByText("認知症の新薬")).toBeInTheDocument();
      await userEvent.click(ask);
      expect(chips.getAllByText("認知症の新薬")).toHaveLength(1);
    });

    it("clears the attachments when the selection changes", async () => {
      setupIpc(feedHandlers());
      render(<App />);
      await waitFor(() => expect(allItem()).toHaveTextContent("7"));
      await userEvent.click(screen.getByRole("button", { name: "フィード" }));
      await userEvent.click(await screen.findByRole("button", { name: "認知症の新薬 についてチャットで質問" }));
      const chips = within(await screen.findByRole("group", { name: "文脈" }));
      expect(chips.getByText("認知症の新薬")).toBeInTheDocument();
      await userEvent.click(screen.getByRole("button", { name: "GitHub" }));
      expect(within(screen.getByRole("group", { name: "文脈" })).queryByText("認知症の新薬")).toBeNull();
    });

    it("opens the LLM settings tab from the panel's error state", async () => {
      setupIpc(handlers({
        chat_endpoint: () => ({ status: "error", kind: "node_missing", message: "Node が見つかりません (node)。" }),
        get_settings: () => ({
          "llm.items_provider": "ollama", "llm.digest_provider": "claude", "llm.chat_provider": "ollama",
          "ollama.base_url": "http://localhost:11434", "ollama.model": "gemma4:26b", "ollama.web_search_api_key": "",
          "claude.bin": "claude", "node.bin": "node", "summary.top_n": "20",
        }),
      }));
      render(<App />);
      await waitFor(() => expect(allItem()).toHaveTextContent("7"));
      await userEvent.click(screen.getByRole("button", { name: "チャットを開く" }));
      await userEvent.click(await screen.findByRole("button", { name: "Node のパスを設定" }));
      expect(await screen.findByLabelText("Node のパス")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "設定" })).toHaveAttribute("aria-current", "true");
    });

    it("keeps the panel and its open thread while settings is shown, hidden", async () => {
      setupIpc(handlers({
        list_sources: () => [], source_item_counts: () => ({}),
        list_chat_threads: () => [{ id: 2, title: "認知症の今週の動き", created_at: "x", updated_at: "x" }],
        get_chat_messages: () => [],
      }));
      render(<App />);
      await waitFor(() => expect(allItem()).toHaveTextContent("7"));
      await userEvent.click(screen.getByRole("button", { name: "チャットを開く" }));
      const el = panel()!;
      await userEvent.click(within(el).getByRole("button", { name: "履歴" }));
      await userEvent.click(await within(el).findByRole("button", { name: "認知症の今週の動き" }));
      await waitFor(() => expect(within(el).getByRole("button", { name: "認知症の今週の動き", expanded: false })).toBeInTheDocument());
      await userEvent.click(screen.getByRole("button", { name: "設定" }));
      expect(await screen.findByRole("button", { name: "RSSを追加" })).toBeInTheDocument();
      expect(panel()).toBeNull();
      expect(el).toBeInTheDocument();
      expect(el).not.toBeVisible();
      await userEvent.click(allItem());
      expect(panel()).toBe(el);
      expect(el).toBeVisible();
      expect(within(el).getByRole("button", { name: "認知症の今週の動き", expanded: false })).toBeInTheDocument();
    });
  });
  describe("keyboard shortcuts", () => {
    const row = (id: number, over: Record<string, unknown> = {}) => ({
      id, source_id: 1, source_name: "s", category: "medcare", url: `https://e.com/${id}`, title: `記事${id}`,
      excerpt: "", author: null, published_at: null, fetched_at: new Date().toISOString(),
      score: null, ai_summary: null, read: false, starred: false, image_url: null, ...over,
    });
    const ROWS = [row(1), row(2), row(3, { read: true, starred: true })];
    const feedHandlers = (over: IpcHandlers = {}) => handlers({
      list_items: () => ROWS, count_items: () => ROWS.length, list_sources: () => [], source_item_counts: () => ({}),
      set_read: () => undefined, set_starred: () => undefined, open_item: () => undefined, mark_all_read: () => 2,
      ...over,
    });
    const press = (k: string, init: KeyboardEventInit = {}, target: Element = document.body) =>
      act(() => { fireEvent.keyDown(target, { key: k, ...init }); });
    const ready = async () => { render(<App />); await waitFor(() => expect(allItem()).toHaveTextContent("7")); };
    const goFeed = async () => {
      await ready();
      await press("2");
      await screen.findByText("記事1");
    };
    const selected = () => document.querySelector(".item-row.selected");

    it("Ctrl+R and Cmd+R start a collection and refetch view, counts and status", async () => {
      const ipc = setupIpc(handlers());
      await ready();
      await waitFor(() => expect(ipc.count("get_digests")).toBe(1));
      await press("r", { ctrlKey: true });
      await waitFor(() => expect(ipc.count("collect_now")).toBe(1));
      await waitFor(() => expect(ipc.count("get_status")).toBe(2));
      expect(ipc.count("sidebar_counts")).toBe(2);
      await waitFor(() => expect(ipc.count("get_digests")).toBe(2));
      await press("r", { metaKey: true });
      await waitFor(() => expect(ipc.count("collect_now")).toBe(2));
      await waitFor(() => expect(ipc.count("get_status")).toBe(3));
    });

    it("prevents the default action of handled keys only", async () => {
      setupIpc(handlers());
      await ready();
      const handled = new KeyboardEvent("keydown", { key: "r", ctrlKey: true, bubbles: true, cancelable: true });
      act(() => { document.body.dispatchEvent(handled); });
      expect(handled.defaultPrevented).toBe(true);
      const ignored = new KeyboardEvent("keydown", { key: "x", bubbles: true, cancelable: true });
      act(() => { document.body.dispatchEvent(ignored); });
      expect(ignored.defaultPrevented).toBe(false);
    });

    it("does not start a collection while one is running, but still refetches", async () => {
      const ipc = setupIpc(handlers({ get_status: () => ({ ...STATUS, collecting: true }) }));
      await ready();
      await screen.findByText("収集中…");
      await press("r", { ctrlKey: true });
      await waitFor(() => expect(ipc.count("get_status")).toBe(2));
      expect(ipc.count("collect_now")).toBe(0);
    });

    it("shows a collect_now failure from the shortcut on the action error line", async () => {
      setupIpc(handlers({ collect_now: () => { throw { kind: "internal", message: "収集を開始できません" }; } }));
      await ready();
      await press("r", { ctrlKey: true });
      expect(await screen.findByRole("alert")).toHaveTextContent("収集を開始できません");
    });

    it("collects from inside the settings view; other keys do nothing there", async () => {
      const ipc = setupIpc(handlers({ list_sources: () => [], source_item_counts: () => ({}) }));
      await ready();
      await press(",", { metaKey: true });
      expect(await screen.findByRole("button", { name: "RSSを追加" })).toBeInTheDocument();
      await press("r", { ctrlKey: true });
      await waitFor(() => expect(ipc.count("collect_now")).toBe(1));
      await press("2");
      await press("[");
      await press("0");
      expect(screen.getByRole("button", { name: "設定" })).toHaveAttribute("aria-current", "true");
      expect(screen.getByRole("button", { name: "RSSを追加" })).toBeInTheDocument();
    });

    it("ignores shortcuts while the add-RSS dialog is open", async () => {
      const ipc = setupIpc(handlers({ list_sources: () => [], source_item_counts: () => ({}) }));
      await ready();
      await press(",", { metaKey: true });
      await userEvent.click(await screen.findByRole("button", { name: "RSSを追加" }));
      const dialog = screen.getByRole("dialog", { name: "RSSフィードを追加" });
      expect(dialog).toHaveAttribute("aria-modal", "true");
      await press("r", { ctrlKey: true }, dialog);
      await press("?", { shiftKey: true }, dialog);
      await new Promise((r) => setTimeout(r, 50));
      expect(ipc.count("collect_now")).toBe(0);
      expect(screen.queryByRole("dialog", { name: "ショートカット" })).toBeNull();
    });

    it("1 and 2 switch between digest and feed", async () => {
      setupIpc(feedHandlers());
      await ready();
      await press("2");
      expect(screen.getByRole("button", { name: "フィード" })).toHaveAttribute("aria-pressed", "true");
      await press("1");
      expect(screen.getByRole("button", { name: "ダイジェスト" })).toHaveAttribute("aria-pressed", "true");
    });

    it("d / w / m change the digest period and the feed range", async () => {
      const ipc = setupIpc(feedHandlers());
      await ready();
      const pressed = (name: string) => within(screen.getByRole("group", { name: "期間" })).getByRole("button", { name }).getAttribute("aria-pressed");
      await press("w");
      expect(pressed("週")).toBe("true");
      await waitFor(() => expect(ipc.last("get_digests").period).toBe("week"));
      await press("m");
      expect(pressed("月")).toBe("true");
      await press("d");
      expect(pressed("今日")).toBe("true");
      await press("2");
      await screen.findByText("記事1");
      await press("m");
      expect(pressed("1ヶ月")).toBe("true");
      await press("d");
      expect(pressed("今日")).toBe("true");
      await press("w");
      expect(pressed("1週間")).toBe("true");
    });

    it("Cmd+J toggles the chat panel and Cmd+, opens settings", async () => {
      setupIpc(handlers({ list_sources: () => [], source_item_counts: () => ({}) }));
      await ready();
      await press("j", { metaKey: true });
      expect(screen.getByRole("complementary", { name: "チャット" })).toBeInTheDocument();
      await press("j", { ctrlKey: true });
      expect(screen.queryByRole("complementary", { name: "チャット" })).toBeNull();
      await press(",", { ctrlKey: true });
      expect(await screen.findByRole("button", { name: "RSSを追加" })).toBeInTheDocument();
    });

    it("j / k move the feed selection, starting at the first row", async () => {
      setupIpc(feedHandlers());
      await goFeed();
      const scroll = vi.fn();
      const original = Object.getOwnPropertyDescriptor(Element.prototype, "scrollIntoView");
      Element.prototype.scrollIntoView = scroll;
      try {
        expect(selected()).toBeNull();
        await press("j");
        expect(selected()).toHaveTextContent("記事1");
        expect(scroll).toHaveBeenLastCalledWith({ block: "nearest" });
        await press("j");
        expect(selected()).toHaveTextContent("記事2");
        await press("j");
        await press("j");
        expect(selected()).toHaveTextContent("記事3");
        await press("k");
        expect(selected()).toHaveTextContent("記事2");
        await press("k");
        await press("k");
        expect(selected()).toHaveTextContent("記事1");
        // Caps Lock 中の大文字でも動く。
        await press("J");
        expect(selected()).toHaveTextContent("記事2");
      } finally {
        if (original) Object.defineProperty(Element.prototype, "scrollIntoView", original);
        else delete (Element.prototype as { scrollIntoView?: unknown }).scrollIntoView;
      }
    });

    it("Enter opens, u toggles read, s toggles star, Shift+A marks all read", async () => {
      const ipc = setupIpc(feedHandlers());
      await goFeed();
      await press("Enter");
      expect(ipc.count("open_item")).toBe(0);
      await press("j");
      await press("Enter");
      await waitFor(() => expect(ipc.last("open_item")).toEqual({ id: 1 }));
      await press("u");
      await waitFor(() => expect(ipc.last("set_read")).toEqual({ id: 1, read: true }));
      await press("s");
      await waitFor(() => expect(ipc.last("set_starred")).toEqual({ id: 1, starred: true }));
      await press("j"); await press("j");
      await press("u");
      await waitFor(() => expect(ipc.last("set_read")).toEqual({ id: 3, read: false }));
      await press("s");
      await waitFor(() => expect(ipc.last("set_starred")).toEqual({ id: 3, starred: false }));
      await press("A", { shiftKey: true });
      await waitFor(() => expect(ipc.count("mark_all_read")).toBe(1));
      expect(ipc.last("mark_all_read").filter.category).toBeNull();
    });

    it("/ focuses the search field and typing there does not trigger shortcuts", async () => {
      setupIpc(feedHandlers());
      await goFeed();
      await press("/");
      const search = screen.getByRole("textbox", { name: "検索" });
      expect(search).toHaveFocus();
      await press("2", {}, search);
      await press("j", {}, search);
      expect(selected()).toBeNull();
      expect(screen.getByRole("button", { name: "フィード" })).toHaveAttribute("aria-pressed", "true");
    });

    it("[ and ] walk all, categories, then topics, wrapping; 0 selects all", async () => {
      const topics: Topic[] = [{ id: 5, name: "認知症", keywords: ["認知症"] }];
      setupIpc(handlers({ list_topics: () => topics }));
      await ready();
      await screen.findByRole("button", { name: /認知症/ });
      await press("]");
      expect(screen.getByRole("button", { name: "GitHub" })).toHaveAttribute("aria-current", "true");
      for (let i = 0; i < 6; i++) await press("]");
      expect(screen.getByRole("button", { name: "医療介護" })).toHaveAttribute("aria-current", "true");
      await press("]");
      expect(screen.getByRole("button", { name: /認知症/ })).toHaveAttribute("aria-current", "true");
      await press("]");
      expect(allItem()).toHaveAttribute("aria-current", "true");
      await press("[");
      expect(screen.getByRole("button", { name: /認知症/ })).toHaveAttribute("aria-current", "true");
      await press("0");
      expect(allItem()).toHaveAttribute("aria-current", "true");
    });

    it("? opens the help overlay, Esc and the close button close it, and it blocks other keys", async () => {
      setupIpc(feedHandlers());
      await ready();
      await press("?", { shiftKey: true });
      const dialog = screen.getByRole("dialog", { name: "ショートカット" });
      expect(dialog).toHaveAttribute("aria-modal", "true");
      await press("2");
      expect(screen.getByRole("button", { name: "ダイジェスト" })).toHaveAttribute("aria-pressed", "true");
      await press("Escape", {}, dialog);
      expect(screen.queryByRole("dialog")).toBeNull();
      await press("?", { shiftKey: true });
      await userEvent.click(screen.getByRole("button", { name: "閉じる" }));
      expect(screen.queryByRole("dialog")).toBeNull();
    });

    it("opens help from the settings view too", async () => {
      setupIpc(handlers({ list_sources: () => [], source_item_counts: () => ({}) }));
      await ready();
      await press(",", { metaKey: true });
      await screen.findByRole("button", { name: "RSSを追加" });
      await press("?", { shiftKey: true });
      expect(screen.getByRole("dialog", { name: "ショートカット" })).toBeInTheDocument();
    });

    it("refetches the view, counts and status when the stale-refresh hook fires", async () => {
      const ipc = setupIpc(handlers());
      await ready();
      await waitFor(() => expect(ipc.count("get_digests")).toBe(1));
      // focus / visibilitychange の配線は useStaleRefresh のテストで確かめる (jsdom が合成イベントで警告を出すため)。
      act(() => { stale.fn!(); });
      await waitFor(() => expect(ipc.count("get_status")).toBe(2));
      await waitFor(() => expect(ipc.count("get_digests")).toBe(2));
      await new Promise((r) => setTimeout(r, 400));
      expect(ipc.count("get_status")).toBe(2);
      expect(ipc.count("sidebar_counts")).toBe(2);
      expect(ipc.count("get_digests")).toBe(2);
    });

    it("moves the digest to the new day when the local date changes", async () => {
      vi.useFakeTimers({ toFake: ["Date", "setInterval", "clearInterval"], now: new Date(2026, 9, 13, 23, 59) });
      try {
        const ipc = setupIpc(handlers());
        await ready();
        await waitFor(() => expect(ipc.last("get_digests")?.date).toBe("2026-10-13"));
        vi.setSystemTime(new Date(2026, 9, 14, 0, 1));
        await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
        await waitFor(() => expect(ipc.last("get_digests").date).toBe("2026-10-14"));
        expect(ipc.last("digest_stats").date).toBe("2026-10-14");
        await act(async () => { await vi.advanceTimersByTimeAsync(0); });
        // 日付の変化と refreshTick の両方で取り直さない (1組だけ)。
        const onNewDay = (cmd: string) => ipc.calls.filter((c) => c.cmd === cmd && c.args.date === "2026-10-14").length;
        expect(onNewDay("get_digests")).toBe(1);
        expect(onNewDay("digest_stats")).toBe(1);
      } finally {
        vi.useRealTimers();
      }
    });
  });
});
