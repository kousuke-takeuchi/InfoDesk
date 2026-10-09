import { describe, expect, it } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import App from "./App";
import { EVENTS } from "./api";
import { emitEvent, setupIpc, type IpcHandlers } from "./test/ipc";
import type { AppStatus, SidebarCounts, Topic } from "./types";

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

    await userEvent.click(screen.getByTitle("今すぐ収集"));
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
      score: null, ai_summary: null, read: false, starred: false,
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
      expect(await screen.findByLabelText("モデル")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "設定" })).toHaveAttribute("aria-current", "true");
    });

    it("hides the panel in settings and brings it back afterwards", async () => {
      setupIpc(handlers({ list_sources: () => [], source_item_counts: () => ({}) }));
      render(<App />);
      await waitFor(() => expect(allItem()).toHaveTextContent("7"));
      await userEvent.click(screen.getByRole("button", { name: "チャットを開く" }));
      expect(panel()).toBeInTheDocument();
      await userEvent.click(screen.getByRole("button", { name: "設定" }));
      expect(await screen.findByRole("button", { name: "RSSを追加" })).toBeInTheDocument();
      expect(panel()).toBeNull();
      await userEvent.click(allItem());
      expect(panel()).toBeInTheDocument();
    });
  });
});
