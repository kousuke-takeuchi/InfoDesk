import { describe, expect, it } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { LlmTab } from "./LlmTab";
import { setupIpc } from "../../test/ipc";

const settings = {
  "llm.items_provider": "ollama", "llm.digest_provider": "claude", "llm.chat_provider": "ollama",
  "ollama.base_url": "http://localhost:11434", "ollama.model": "gemma4:26b", "ollama.web_search_api_key": "",
  "claude.bin": "claude", "node.bin": "node", "summary.top_n": "20",
};

function deferred<T = void>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => { resolve = r; });
  return { promise, resolve };
}

describe("LlmTab", () => {
  it("shows providers and saves changes", async () => {
    const ipc = setupIpc({ get_settings: () => settings, set_setting: () => undefined });
    render(<LlmTab />);
    expect(await screen.findByLabelText("期間要約")).toHaveValue("claude");
    await userEvent.selectOptions(screen.getByLabelText("記事要約"), "claude");
    expect(ipc.last("set_setting")).toEqual({ key: "llm.items_provider", value: "claude" });

    const model = screen.getByLabelText("モデル");
    await userEvent.clear(model);
    await userEvent.type(model, "gemma4:12b{enter}");
    expect(ipc.last("set_setting")).toEqual({ key: "ollama.model", value: "gemma4:12b" });
  });

  it("runs connection tests and shows the result or the error", async () => {
    setupIpc({
      get_settings: () => settings,
      test_ollama: () => ({ ok: true, message: "接続済み (http://localhost:11434/api/tags)", millis: 800 }),
      test_claude: () => ({ ok: false, message: "claude: Not logged in", millis: 5000 }),
    });
    render(<LlmTab />);
    await screen.findByLabelText("モデル");
    await userEvent.click(screen.getByRole("button", { name: "Ollama をテスト" }));
    expect(await screen.findByText(/接続済み/)).toBeInTheDocument();
    expect(screen.getByText(/0\.8秒/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Claude をテスト" }));
    expect(await screen.findByText(/Not logged in/)).toBeInTheDocument();
  });

  it("shows the backend validation error for a bad path and keeps the typed value", async () => {
    setupIpc({
      get_settings: () => settings,
      set_setting: (a) => { if (a.key === "claude.bin") throw { message: "claude.bin: not an existing file" }; },
    });
    render(<LlmTab />);
    const bin = await screen.findByLabelText("claude コマンドのパス");
    await userEvent.clear(bin);
    await userEvent.type(bin, "/nope/claude{enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent("not an existing file");
    expect(screen.getByLabelText("claude コマンドのパス")).toHaveValue("/nope/claude");
  });

  it("clears the action error on the next successful save", async () => {
    let fail = true;
    setupIpc({
      get_settings: () => settings,
      set_setting: (a) => { if (fail && a.key === "claude.bin") throw { message: "claude.bin: not an existing file" }; },
    });
    render(<LlmTab />);
    const bin = await screen.findByLabelText("claude コマンドのパス");
    await userEvent.clear(bin);
    await userEvent.type(bin, "/nope/claude{enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent("not an existing file");

    fail = false;
    await userEvent.selectOptions(screen.getByLabelText("チャット"), "claude");
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
  });

  it("keeps the form and shows the load error when a reload fails after a successful save", async () => {
    let n = 0;
    setupIpc({
      get_settings: () => { if (++n > 1) throw { message: "db locked" }; return settings; },
      set_setting: () => undefined,
    });
    render(<LlmTab />);
    await userEvent.selectOptions(await screen.findByLabelText("記事要約"), "claude");
    expect(await screen.findByRole("alert")).toHaveTextContent("db locked");
    expect(screen.getByLabelText("モデル")).toBeInTheDocument();
  });

  it("shows a load error from get_settings", async () => {
    setupIpc({ get_settings: () => { throw { message: "db locked" }; } });
    render(<LlmTab />);
    expect(await screen.findByRole("alert")).toHaveTextContent("db locked");
  });

  it("disables a probe button while its request is in flight", async () => {
    const gate = deferred<{ ok: boolean; message: string; millis: number }>();
    const ipc = setupIpc({ get_settings: () => settings, test_ollama: () => gate.promise });
    render(<LlmTab />);
    await screen.findByLabelText("モデル");
    await userEvent.click(screen.getByRole("button", { name: "Ollama をテスト" }));
    const busy = await screen.findByRole("button", { name: /テスト中/ });
    expect(busy).toBeDisabled();
    await userEvent.click(busy);
    expect(ipc.count("test_ollama")).toBe(1);
    await act(async () => gate.resolve({ ok: true, message: "接続済み", millis: 100 }));
    expect(await screen.findByText(/接続済み/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Ollama をテスト" })).toBeEnabled();
  });

  it("renders the web search API key as a password field and never as text", async () => {
    setupIpc({ get_settings: () => ({ ...settings, "ollama.web_search_api_key": "sk-secret-123" }) });
    const { container } = render(<LlmTab />);
    const key = await screen.findByLabelText("Web検索 APIキー");
    expect(key).toHaveAttribute("type", "password");
    expect(key).toHaveValue("sk-secret-123");
    expect(container.textContent).not.toContain("sk-secret-123");
  });
});
