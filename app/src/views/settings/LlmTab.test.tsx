import { describe, expect, it } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
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

  it("keeps a failed field's inline error and typed value after an unrelated action succeeds", async () => {
    setupIpc({
      get_settings: () => settings,
      set_setting: (a) => { if (a.key === "claude.bin") throw { message: "claude.bin: not an existing file" }; },
    });
    render(<LlmTab />);
    const bin = await screen.findByLabelText("claude コマンドのパス");
    await userEvent.clear(bin);
    await userEvent.type(bin, "/nope/claude{enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent("not an existing file");
    expect(bin).toHaveAttribute("aria-invalid", "true");
    expect(bin).toHaveAccessibleDescription(/not an existing file/);

    await userEvent.selectOptions(screen.getByLabelText("チャット"), "claude");
    // 共有の actionError は次の成功で消えるが、欄の横のエラーと入力値は残る
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
    expect(screen.getByText(/not an existing file/)).toBeInTheDocument();
    expect(bin).toHaveAttribute("aria-invalid", "true");
    expect(bin).toHaveValue("/nope/claude");
  });

  it("clears a field's inline error when the draft returns to the stored value", async () => {
    setupIpc({ get_settings: () => settings, set_setting: () => { throw { message: "storage failed" }; } });
    render(<LlmTab />);
    const model = await screen.findByLabelText("モデル");
    await userEvent.clear(model);
    await userEvent.type(model, "x{enter}");
    await waitFor(() => expect(model).toHaveAttribute("aria-invalid", "true"));
    await userEvent.clear(model);
    await userEvent.type(model, "gemma4:26b");
    expect(model).not.toHaveAttribute("aria-invalid");
    expect(model).not.toHaveAccessibleDescription();
  });

  it("clears a field's inline error when that field saves successfully", async () => {
    let fail = true;
    setupIpc({
      get_settings: () => settings,
      set_setting: () => { if (fail) throw { message: "storage failed" }; },
    });
    render(<LlmTab />);
    const model = await screen.findByLabelText("モデル");
    await userEvent.clear(model);
    await userEvent.type(model, "x{enter}");
    await waitFor(() => expect(model).toHaveAttribute("aria-invalid", "true"));
    fail = false;
    await userEvent.type(model, "y{enter}");
    await waitFor(() => expect(model).not.toHaveAttribute("aria-invalid"));
    expect(screen.queryByText(/storage failed/)).not.toBeInTheDocument();
  });

  it("does not save on Enter while an IME composition is active", async () => {
    const ipc = setupIpc({ get_settings: () => settings, set_setting: () => undefined });
    render(<LlmTab />);
    const model = await screen.findByLabelText("モデル");
    fireEvent.change(model, { target: { value: "がんま" } });
    fireEvent.keyDown(model, { key: "Enter", isComposing: true });
    expect(ipc.count("set_setting")).toBe(0);
    fireEvent.keyDown(model, { key: "Enter" });
    await waitFor(() => expect(ipc.last("set_setting")).toEqual({ key: "ollama.model", value: "がんま" }));
  });

  it("shows a stored provider outside the option list as itself and does not change it", async () => {
    const ipc = setupIpc({ get_settings: () => ({ ...settings, "llm.chat_provider": "openai" }), set_setting: () => undefined });
    render(<LlmTab />);
    const chat = await screen.findByLabelText("チャット");
    expect(chat).toHaveValue("openai");
    expect(screen.getByRole("option", { name: "openai" })).toHaveProperty("selected", true);
    expect(ipc.count("set_setting")).toBe(0);
    await userEvent.selectOptions(chat, "claude");
    expect(ipc.last("set_setting")).toEqual({ key: "llm.chat_provider", value: "claude" });
  });

  it("clears the previous probe result while a new probe runs", async () => {
    const gate = deferred<{ ok: boolean; message: string; millis: number }>();
    let n = 0;
    setupIpc({
      get_settings: () => settings,
      test_ollama: () => (++n === 1 ? { ok: true, message: "接続済み", millis: 800 } : gate.promise),
    });
    render(<LlmTab />);
    await screen.findByLabelText("モデル");
    await userEvent.click(screen.getByRole("button", { name: "Ollama をテスト" }));
    expect(await screen.findByText(/接続済み/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Ollama をテスト" }));
    expect(await screen.findByRole("button", { name: /テスト中…/ })).toBeDisabled();
    expect(screen.queryByText(/接続済み/)).not.toBeInTheDocument();
    await act(async () => gate.resolve({ ok: false, message: "接続できません", millis: 1200 }));
    expect(await screen.findByText(/接続できません · 1\.2秒/)).toBeInTheDocument();
  });

  it("shows a rejected probe's error without a duration", async () => {
    setupIpc({ get_settings: () => settings, test_claude: () => { throw { message: "spawn failed" }; } });
    render(<LlmTab />);
    await screen.findByLabelText("モデル");
    await userEvent.click(screen.getByRole("button", { name: "Claude をテスト" }));
    const msg = await screen.findByText(/spawn failed/);
    expect(msg).not.toHaveTextContent("秒");
    expect(msg).not.toHaveTextContent("·");
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
