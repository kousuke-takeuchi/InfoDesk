import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { emitEvent, setupIpc } from "../test/ipc";
import { ChatPanel } from "./ChatPanel";

let lastThreadProps: any = null;
vi.mock("./ChatThread", () => ({
  ChatThread: (p: any) => {
    lastThreadProps = p;
    return <div data-testid="thread">{p.chatKey}|{p.initialMessages.length}|{p.provider}</div>;
  },
}));

const READY = { status: "ready", url: "http://127.0.0.1:4321", token: "tok" };
const SETTINGS = { "llm.chat_provider": "ollama", "ollama.model": "gemma4:26b", "claude.model": "sonnet" };
const THREADS = [
  { id: 2, title: "認知症の今週の動き", created_at: "2026-10-08T00:00:00Z", updated_at: "2026-10-09T00:00:00Z" },
  { id: 1, title: "GitHub の話題", created_at: "2026-10-07T00:00:00Z", updated_at: "2026-10-07T00:00:00Z" },
];

function base(over: Record<string, (a: any) => unknown> = {}) {
  return setupIpc({
    chat_endpoint: () => READY,
    get_settings: () => SETTINGS,
    ollama_models: () => ["gemma4:26b", "qwen3:8b"],
    set_setting: () => null,
    list_chat_threads: () => THREADS,
    get_chat_messages: () => [{ id: "u", role: "user", parts: [{ type: "text", text: "x" }] }],
    ...over,
  });
}

function renderPanel(over: Partial<React.ComponentProps<typeof ChatPanel>> = {}) {
  const props = {
    scope: { kind: "category", category: "medcare", label: "医療介護" } as const,
    period: { view: "feed", range: "week" } as const,
    attachments: [],
    onRemoveAttachment: vi.fn(), onClose: vi.fn(), onOpenSettings: vi.fn(),
    ...over,
  };
  render(<ChatPanel {...(props as any)} />);
  return props;
}

describe("ChatPanel", () => {
  it("starts on a new thread with the context chips and the default provider", async () => {
    base();
    renderPanel();
    expect(await screen.findByTestId("thread")).toHaveTextContent("|0|ollama");
    expect(screen.getByRole("button", { name: "新しいスレッド" })).toHaveAttribute("aria-expanded", "false");
    const chips = screen.getByRole("group", { name: "文脈" });
    expect(within(chips).getByText("医療介護")).toBeInTheDocument();
    expect(within(chips).getByText("期間: 1週間")).toBeInTheDocument();
    expect(lastThreadProps.ollamaModel).toBe("gemma4:26b");
    expect(lastThreadProps.context).toEqual({ scope: { kind: "category", category: "medcare", label: "医療介護" }, period: { view: "feed", range: "week" }, attachments: [] });
  });

  it("creates the thread on the first save, titled from the question", async () => {
    const ipc = base({ create_chat_thread: (a) => ({ id: 9, title: a.title, created_at: "x", updated_at: "x" }), save_chat_messages: () => null });
    renderPanel();
    await screen.findByTestId("thread");
    const long = "今週の認知症に関する記事をまとめて。新薬と介護現場の話題を分けて、それぞれ3行で";
    await lastThreadProps.onPersist([
      { id: "u", role: "user", parts: [{ type: "text", text: long }] },
      { id: "a", role: "assistant", parts: [{ type: "text", text: "..." }] },
    ]);
    expect(ipc.last("create_chat_thread")).toEqual({ title: long.slice(0, 30) });
    expect(ipc.last("save_chat_messages").threadId).toBe(9);
    // 2回目は作らずに保存だけ
    await lastThreadProps.onPersist([]);
    expect(ipc.count("create_chat_thread")).toBe(1);
    expect(ipc.count("save_chat_messages")).toBe(2);
  });

  it("switches to a saved thread and deletes one after confirming", async () => {
    const ipc = base({ delete_chat_thread: () => null });
    renderPanel();
    await screen.findByTestId("thread");
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "履歴" }));
    await user.click(screen.getByRole("button", { name: "認知症の今週の動き" }));
    await waitFor(() => expect(screen.getByTestId("thread")).toHaveTextContent("thread-2|1|"));
    expect(ipc.last("get_chat_messages")).toEqual({ threadId: 2 });
    await user.click(screen.getByRole("button", { name: "履歴" }));
    await user.click(screen.getByRole("button", { name: "認知症の今週の動き を削除" }));
    expect(ipc.count("delete_chat_thread")).toBe(0);
    await user.click(screen.getByRole("button", { name: "削除する" }));
    expect(ipc.last("delete_chat_thread")).toEqual({ id: 2 });
    // 開いていたスレッドを消したら新しいスレッドに戻る
    await waitFor(() => expect(screen.getByTestId("thread")).toHaveTextContent("|0|"));
  });

  it("starts a new thread with the new-thread button", async () => {
    base();
    renderPanel();
    await screen.findByTestId("thread");
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "履歴" }));
    await user.click(screen.getByRole("button", { name: "GitHub の話題" }));
    await waitFor(() => expect(screen.getByTestId("thread")).toHaveTextContent("thread-1"));
    await user.click(screen.getByRole("button", { name: "新規スレッド" }));
    expect(screen.getByTestId("thread")).toHaveTextContent("|0|");
  });

  it("shows why the agent cannot start, with settings and retry", async () => {
    const ipc = base({
      chat_endpoint: () => ({ status: "error", kind: "node_missing", message: "Node が見つかりません (node)。" }),
      restart_chat: () => null,
    });
    const props = renderPanel();
    expect(await screen.findByText("エージェントを起動できません")).toBeInTheDocument();
    expect(screen.getByText("Node が見つかりません (node)。")).toBeInTheDocument();
    expect(screen.queryByTestId("thread")).toBeNull();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Node のパスを設定" }));
    expect(props.onOpenSettings).toHaveBeenCalledWith("llm");
    await user.click(screen.getByRole("button", { name: "再試行" }));
    expect(ipc.count("restart_chat")).toBe(1);
  });

  it("follows endpoint changes", async () => {
    let endpoint: any = { status: "starting" };
    base({ chat_endpoint: () => endpoint });
    renderPanel();
    expect(await screen.findByText("エージェントを起動しています…")).toBeInTheDocument();
    endpoint = READY;
    await emitEvent("chat-endpoint-changed");
    expect(await screen.findByTestId("thread")).toBeInTheDocument();
  });

  it("lists and removes attachments", async () => {
    base();
    const props = renderPanel({ attachments: [{ type: "item", id: 12, title: "認知症の新薬" }] as any });
    await screen.findByTestId("thread");
    await userEvent.setup().click(screen.getByRole("button", { name: "認知症の新薬 を文脈から外す" }));
    expect(props.onRemoveAttachment).toHaveBeenCalledWith(0);
  });

  it("closes", async () => {
    base();
    const props = renderPanel();
    await userEvent.setup().click(screen.getByRole("button", { name: "チャットを閉じる" }));
    expect(props.onClose).toHaveBeenCalled();
  });
  it("passes a restart action to the thread that restarts the sidecar", async () => {
    const ipc = base({ restart_chat: () => null });
    renderPanel();
    await screen.findByTestId("thread");
    expect(typeof lastThreadProps.onRestartAgent).toBe("function");
    act(() => lastThreadProps.onRestartAgent([]));
    await waitFor(() => expect(ipc.count("restart_chat")).toBe(1));
  });

  it("keeps the saved history of the open thread across an agent restart", async () => {
    let endpoint: any = READY;
    const ipc = base({
      chat_endpoint: () => endpoint,
      create_chat_thread: (a) => ({ id: 9, title: a.title, created_at: "x", updated_at: "x" }),
      save_chat_messages: () => null,
      restart_chat: () => null,
    });
    renderPanel();
    await screen.findByTestId("thread");
    const two = [
      { id: "u", role: "user", parts: [{ type: "text", text: "質問" }] },
      { id: "a", role: "assistant", parts: [{ type: "text", text: "回答" }] },
    ];
    await act(async () => { await lastThreadProps.onPersist(two); });
    endpoint = { status: "starting" };
    act(() => lastThreadProps.onRestartAgent(lastThreadProps.initialMessages));
    expect(await screen.findByText("エージェントを起動しています…")).toBeInTheDocument();
    endpoint = READY;
    await emitEvent("chat-endpoint-changed");
    await screen.findByTestId("thread");
    expect(lastThreadProps.initialMessages).toEqual(two);
    const three = [...lastThreadProps.initialMessages, { id: "u2", role: "user", parts: [{ type: "text", text: "続き" }] }];
    await act(async () => { await lastThreadProps.onPersist(three); });
    expect(ipc.count("create_chat_thread")).toBe(1);
    expect(ipc.last("save_chat_messages")).toEqual({ threadId: 9, messages: three });
  });

  it("keeps the unsent question across an agent restart without saving it", async () => {
    let endpoint: any = READY;
    const ipc = base({ chat_endpoint: () => endpoint, restart_chat: () => null });
    renderPanel();
    await screen.findByTestId("thread");
    const key = lastThreadProps.chatKey;
    const failed = [{ id: "u", role: "user", parts: [{ type: "text", text: "未送信の質問" }] }];
    endpoint = { status: "starting" };
    act(() => lastThreadProps.onRestartAgent(failed));
    expect(await screen.findByText("エージェントを起動しています…")).toBeInTheDocument();
    await waitFor(() => expect(ipc.count("restart_chat")).toBe(1));
    endpoint = READY;
    await emitEvent("chat-endpoint-changed");
    await screen.findByTestId("thread");
    expect(lastThreadProps.chatKey).toBe(key);
    expect(lastThreadProps.initialMessages).toEqual(failed);
    expect(ipc.count("save_chat_messages")).toBe(0);
    expect(ipc.count("create_chat_thread")).toBe(0);
  });

  it("saves a late answer into the thread it belongs to, not the one now open", async () => {
    const ipc = base({ save_chat_messages: () => null });
    renderPanel();
    await screen.findByTestId("thread");
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "履歴" }));
    await user.click(screen.getByRole("button", { name: "認知症の今週の動き" }));
    await waitFor(() => expect(screen.getByTestId("thread")).toHaveTextContent("thread-2|"));
    const persistA = lastThreadProps.onPersist;
    await user.click(screen.getByRole("button", { name: "履歴" }));
    await user.click(screen.getByRole("button", { name: "GitHub の話題" }));
    await waitFor(() => expect(screen.getByTestId("thread")).toHaveTextContent("thread-1|"));
    await act(async () => { await persistA([{ id: "a", role: "assistant", parts: [] }]); });
    expect(ipc.last("save_chat_messages").threadId).toBe(2);
  });

  it("saves into the new thread it created even if another thread was opened meanwhile", async () => {
    let resolveCreate!: (v: unknown) => void;
    const ipc = base({
      create_chat_thread: () => new Promise((r) => { resolveCreate = r; }),
      save_chat_messages: () => null,
    });
    renderPanel();
    await screen.findByTestId("thread");
    const persistNew = lastThreadProps.onPersist;
    let pending!: Promise<void>;
    act(() => { pending = persistNew([{ id: "u", role: "user", parts: [{ type: "text", text: "q" }] }]); });
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "履歴" }));
    await user.click(screen.getByRole("button", { name: "GitHub の話題" }));
    await waitFor(() => expect(screen.getByTestId("thread")).toHaveTextContent("thread-1|"));
    await act(async () => { resolveCreate({ id: 9, title: "q", created_at: "x", updated_at: "x" }); await pending; });
    expect(ipc.last("save_chat_messages").threadId).toBe(9);
    expect(screen.getByTestId("thread")).toHaveTextContent("thread-1|");
  });

  it("applies only the last opened thread when loads resolve out of order", async () => {
    const resolvers: Record<number, (v: unknown) => void> = {};
    base({ get_chat_messages: (a) => new Promise((r) => { resolvers[a.threadId] = r; }) });
    renderPanel();
    await screen.findByTestId("thread");
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "履歴" }));
    await user.click(screen.getByRole("button", { name: "認知症の今週の動き" }));
    await user.click(screen.getByRole("button", { name: "履歴" }));
    await user.click(screen.getByRole("button", { name: "GitHub の話題" }));
    await waitFor(() => expect(Object.keys(resolvers)).toHaveLength(2));
    await act(async () => { resolvers[1]([]); });
    await waitFor(() => expect(screen.getByTestId("thread")).toHaveTextContent("thread-1|0|"));
    await act(async () => { resolvers[2]([{ id: "u", role: "user", parts: [] }]); });
    expect(screen.getByTestId("thread")).toHaveTextContent("thread-1|0|");
  });

  it("re-reads the chat settings when the panel is shown again", async () => {
    let settings: Record<string, string> = SETTINGS;
    base({ get_settings: () => settings });
    const props = {
      scope: { kind: "all" } as const, period: null, attachments: [],
      onRemoveAttachment: vi.fn(), onClose: vi.fn(), onOpenSettings: vi.fn(),
    };
    const { rerender } = render(<ChatPanel {...(props as any)} hidden={false} />);
    await waitFor(() => expect(lastThreadProps?.ollamaModel).toBe("gemma4:26b"));
    settings = { "llm.chat_provider": "claude", "ollama.model": "qwen3:8b" };
    rerender(<ChatPanel {...(props as any)} hidden />);
    rerender(<ChatPanel {...(props as any)} hidden={false} />);
    await waitFor(() => expect(lastThreadProps.ollamaModel).toBe("qwen3:8b"));
    expect(lastThreadProps.provider).toBe("claude");
    expect(screen.getByTestId("thread")).toHaveTextContent("|claude");
  });

  it("passes the model lists and writes the shared setting when a model is picked", async () => {
    const ipc = base();
    renderPanel();
    await waitFor(() => expect(lastThreadProps?.ollamaModels).toEqual(["gemma4:26b", "qwen3:8b"]));
    expect(lastThreadProps.claudeModel).toBe("sonnet");
    act(() => lastThreadProps.onModelChange("claude", "opus"));
    await waitFor(() => expect(ipc.last("set_setting")).toEqual({ key: "claude.model", value: "opus" }));
    await waitFor(() => expect(lastThreadProps.provider).toBe("claude"));
    expect(lastThreadProps.claudeModel).toBe("opus");
    act(() => lastThreadProps.onModelChange("ollama", "qwen3:8b"));
    await waitFor(() => expect(ipc.last("set_setting")).toEqual({ key: "ollama.model", value: "qwen3:8b" }));
    expect(lastThreadProps.provider).toBe("ollama");
    expect(lastThreadProps.ollamaModel).toBe("qwen3:8b");
  });

  it("passes an empty Ollama list when the models cannot be fetched", async () => {
    base({ ollama_models: () => { throw { message: "refused" }; } });
    renderPanel();
    await waitFor(() => expect(lastThreadProps?.ollamaModel).toBe("gemma4:26b"));
    expect(lastThreadProps.ollamaModels).toEqual([]);
  });

  it("drops a late save for a thread opened from history and then deleted", async () => {
    const ipc = base({ delete_chat_thread: () => null, save_chat_messages: () => null, create_chat_thread: () => ({ id: 99, title: "x", created_at: "x", updated_at: "x" }) });
    renderPanel();
    await screen.findByTestId("thread");
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "履歴" }));
    await user.click(screen.getByRole("button", { name: "認知症の今週の動き" }));
    await waitFor(() => expect(screen.getByTestId("thread")).toHaveTextContent("thread-2|"));
    const persistA = lastThreadProps.onPersist;
    await user.click(screen.getByRole("button", { name: "履歴" }));
    await user.click(screen.getByRole("button", { name: "認知症の今週の動き を削除" }));
    await user.click(screen.getByRole("button", { name: "削除する" }));
    await waitFor(() => expect(screen.getByTestId("thread")).toHaveTextContent("new-"));
    await act(async () => { await persistA([{ id: "a", role: "assistant", parts: [] }]); });
    expect(ipc.count("save_chat_messages")).toBe(0);
    expect(ipc.count("create_chat_thread")).toBe(0);
  });

  it("drops a late save for a new thread that was deleted after it was created", async () => {
    let threads = [...THREADS];
    const ipc = base({
      list_chat_threads: () => threads,
      create_chat_thread: (a) => { const t = { id: 9, title: a.title, created_at: "x", updated_at: "x" }; threads = [t, ...threads]; return t; },
      save_chat_messages: () => null,
      delete_chat_thread: () => null,
    });
    renderPanel();
    await screen.findByTestId("thread");
    const persistNew = lastThreadProps.onPersist;
    const msgs = [{ id: "u", role: "user", parts: [{ type: "text", text: "新しい質問" }] }];
    await act(async () => { await persistNew(msgs); });
    expect(ipc.count("save_chat_messages")).toBe(1);
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "履歴" }));
    await user.click(await screen.findByRole("button", { name: "新しい質問 を削除" }));
    await user.click(screen.getByRole("button", { name: "削除する" }));
    await waitFor(() => expect(ipc.last("delete_chat_thread")).toEqual({ id: 9 }));
    await act(async () => { await persistNew(msgs); });
    expect(ipc.count("save_chat_messages")).toBe(1);
    expect(ipc.count("create_chat_thread")).toBe(1);
  });
});
