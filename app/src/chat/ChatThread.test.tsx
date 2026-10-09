import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { UIMessage } from "ai";
import { afterEach, describe, expect, it, vi } from "vitest";
import { setupIpc } from "../test/ipc";
import { answerChunks, mockFetch, streamResponse } from "../test/stream";
import { ChatThread } from "./ChatThread";
import type { ChatContext } from "./context";

const CONTEXT: ChatContext = { scope: { kind: "category", category: "medcare", label: "医療介護" }, period: { view: "feed", range: "week" }, attachments: [] };
const ENDPOINT = { url: "http://127.0.0.1:4321", token: "tok" };

function renderThread(over: Partial<React.ComponentProps<typeof ChatThread>> = {}) {
  const props = {
    endpoint: ENDPOINT, chatKey: "new-1", initialMessages: [] as UIMessage[], provider: "ollama" as const,
    modelLabel: "Ollama · gemma4:26b", context: CONTEXT,
    onProviderChange: vi.fn(), onPersist: vi.fn(async () => {}), onOpenSettings: vi.fn(),
    suggestions: ["今日のGitHubで伸びたリポジトリを3つ選んで理由を説明して"],
    ...over,
  };
  render(<ChatThread {...props} />);
  return props;
}

async function ask(text: string) {
  const user = userEvent.setup();
  await user.type(screen.getByRole("textbox", { name: "質問" }), text);
  await user.click(screen.getByRole("button", { name: "送信" }));
}

afterEach(() => vi.unstubAllGlobals());

describe("ChatThread", () => {
  it("sends the question with the provider, the context and the token", async () => {
    setupIpc({});
    const calls = mockFetch(() => streamResponse(answerChunks("答えです。")));
    renderThread();
    await ask("今週の認知症の動きは?");
    expect(await screen.findByText("答えです。")).toBeInTheDocument();
    expect(calls[0].url).toBe("http://127.0.0.1:4321/chat");
    expect(new Headers(calls[0].init.headers).get("authorization")).toBe("Bearer tok");
    expect(calls[0].body.provider).toBe("ollama");
    expect(calls[0].body.context).toEqual(CONTEXT);
    expect(calls[0].body.messages.at(-1).parts[0].text).toBe("今週の認知症の動きは?");
  });

  it("shows tool lines, markdown and the referenced articles", async () => {
    const ipc = setupIpc({ open_item: () => null });
    mockFetch(() => streamResponse(answerChunks("## 新薬\n- 適用拡大の検討", [
      { type: "tool-input-available", toolCallId: "c1", toolName: "search_articles", input: { query: "認知症", days: 7 } },
      { type: "tool-output-available", toolCallId: "c1", output: { items: [{ id: 12, title: "認知症の新薬、適用拡大へ" }], total: 14 } },
    ] as never)));
    renderThread();
    await ask("まとめて");
    expect(await screen.findByText("記事検索")).toBeInTheDocument();
    expect(screen.getByText("認知症 · 直近7日 · 14件")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "新薬" })).toBeInTheDocument();
    expect(screen.getByText("適用拡大の検討")).toBeInTheDocument();
    await userEvent.setup().click(screen.getByRole("button", { name: "認知症の新薬、適用拡大へ" }));
    expect(ipc.last("open_item")).toEqual({ id: 12 });
  });

  it("persists the conversation after an answer completes", async () => {
    setupIpc({});
    mockFetch(() => streamResponse(answerChunks("答え")));
    const props = renderThread();
    await ask("質問");
    await waitFor(() => expect(props.onPersist).toHaveBeenCalled());
    const saved = (props.onPersist as any).mock.calls.at(-1)[0] as UIMessage[];
    expect(saved.map((m) => m.role)).toEqual(["user", "assistant"]);
  });

  it("shows an LLM error with retry, switch provider and settings, keeping the question", async () => {
    setupIpc({});
    const errorText = JSON.stringify({ kind: "llm_unreachable", provider: "ollama", message: "http://localhost:11434 が応答しません。" });
    const calls = mockFetch(
      () => streamResponse([{ type: "start" }, { type: "error", errorText }] as never),
      () => streamResponse(answerChunks("今度は答えた")),
    );
    const props = renderThread();
    await ask("質問A");
    expect(await screen.findByText("Ollama に接続できません")).toBeInTheDocument();
    expect(screen.getByText("http://localhost:11434 が応答しません。")).toBeInTheDocument();
    expect(screen.getByText("質問A")).toBeInTheDocument();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "LLM設定" }));
    expect(props.onOpenSettings).toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "再試行" }));
    expect(await screen.findByText("今度は答えた")).toBeInTheDocument();
    expect(calls).toHaveLength(2);
    expect(calls[1].body.messages.at(-1).parts[0].text).toBe("質問A");
  });

  it("offers to send with the other provider only when the user asks", async () => {
    setupIpc({});
    const errorText = JSON.stringify({ kind: "llm_unreachable", provider: "ollama", message: "x" });
    const calls = mockFetch(() => streamResponse([{ type: "start" }, { type: "error", errorText }] as never));
    const props = renderThread();
    await ask("質問B");
    await screen.findByText("Ollama に接続できません");
    expect(calls).toHaveLength(1);
    await userEvent.setup().click(screen.getByRole("button", { name: "Claude で送る" }));
    expect(props.onProviderChange).toHaveBeenCalledWith("claude");
  });

  it("renders a saved conversation and the empty state", async () => {
    setupIpc({});
    const saved: UIMessage[] = [
      { id: "u", role: "user", parts: [{ type: "text", text: "前の質問" }] },
      { id: "a", role: "assistant", parts: [{ type: "text", text: "前の回答" }] },
    ];
    renderThread({ initialMessages: saved, chatKey: "thread-3" });
    expect(screen.getByText("前の質問")).toBeInTheDocument();
    expect(screen.getByText("前の回答")).toBeInTheDocument();
  });

  it("shows suggestions when empty and sends one on click", async () => {
    setupIpc({});
    const calls = mockFetch(() => streamResponse(answerChunks("はい")));
    renderThread();
    expect(screen.getByText("収集した記事について質問できます")).toBeInTheDocument();
    await userEvent.setup().click(screen.getByRole("button", { name: /今日のGitHubで伸びた/ }));
    await waitFor(() => expect(calls).toHaveLength(1));
    expect(calls[0].body.messages[0].parts[0].text).toContain("今日のGitHubで伸びた");
  });

  it("shows the model and lets the user switch provider", async () => {
    setupIpc({});
    const props = renderThread();
    expect(screen.getByRole("combobox", { name: "LLM" })).toHaveValue("ollama");
    await userEvent.setup().selectOptions(screen.getByRole("combobox", { name: "LLM" }), "claude");
    expect(props.onProviderChange).toHaveBeenCalledWith("claude");
  });

  it("shows a restart button instead of the provider switch when the agent is unreachable", async () => {
    setupIpc({});
    const calls = mockFetch(
      () => { throw new TypeError("Failed to fetch"); },
      () => streamResponse(answerChunks("再起動後の答え")),
    );
    const onRestartAgent = vi.fn();
    renderThread({ onRestartAgent });
    await ask("質問C");
    expect(await screen.findByText("エージェントに接続できません")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /で送る/ })).not.toBeInTheDocument();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "エージェントを再起動" }));
    expect(onRestartAgent).toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "再試行" }));
    expect(await screen.findByText("再起動後の答え")).toBeInTheDocument();
    expect(calls).toHaveLength(2);
  });
});
