import { describe, expect, it } from "vitest";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { LlmTab } from "./LlmTab";
import { setupIpc } from "../../test/ipc";

const settings = {
  "llm.items_provider": "ollama", "llm.digest_provider": "claude", "llm.chat_provider": "ollama",
  "ollama.base_url": "http://localhost:11434", "ollama.model": "gemma4:26b", "ollama.web_search_api_key": "",
  "claude.bin": "claude", "claude.model": "sonnet", "node.bin": "node", "summary.top_n": "20",
};

/** Ollama と Claude の両方に「モデル」欄があるので、セクションで絞る。 */
const ollamaSection = () => within(screen.getByRole("region", { name: "Ollama" }));
const claudeSection = () => within(screen.getByRole("region", { name: "Claude" }));
const findOllamaModel = async () => { await screen.findByRole("region", { name: "Ollama" }); return ollamaSection().getByLabelText("モデル"); };

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

    const model = ollamaSection().getByLabelText("モデル");
    await userEvent.clear(model);
    await userEvent.type(model, "gemma4:12b{enter}");
    expect(ipc.last("set_setting")).toEqual({ key: "ollama.model", value: "gemma4:12b" });
  });

  describe("Ollama model select", () => {
    it("lists installed models, keeps an unlisted stored value, saves a choice and reloads", async () => {
      let models = ["gemma4:26b", "qwen3:8b"];
      const ipc = setupIpc({ get_settings: () => ({ ...settings, "ollama.model": "old:1b" }), set_setting: () => undefined, ollama_models: () => models });
      render(<LlmTab />);
      await screen.findByRole("region", { name: "Ollama" });
      const select = await waitFor(() => {
        const el = ollamaSection().getByLabelText("モデル");
        expect(el.tagName).toBe("SELECT");
        return el;
      });
      expect(select).toHaveValue("old:1b");
      expect(within(select).getAllByRole("option").map((o) => o.textContent)).toEqual(["gemma4:26b", "qwen3:8b", "old:1b"]);
      await userEvent.selectOptions(select, "qwen3:8b");
      expect(ipc.last("set_setting")).toEqual({ key: "ollama.model", value: "qwen3:8b" });

      models = ["gemma4:26b", "qwen3:8b", "llama3:8b"];
      await userEvent.click(ollamaSection().getByRole("button", { name: "再読み込み" }));
      expect(await ollamaSection().findByRole("option", { name: "llama3:8b" })).toBeInTheDocument();
      expect(ipc.count("ollama_models")).toBe(2);
    });

    it("falls back to a text input with a hint when Ollama is unreachable", async () => {
      setupIpc({ get_settings: () => settings, ollama_models: () => { throw { message: "refused" }; } });
      render(<LlmTab />);
      expect(await screen.findByText("Ollama に接続できないため、モデル名を入力してください")).toBeInTheDocument();
      const input = ollamaSection().getByLabelText("モデル");
      expect(input.tagName).toBe("INPUT");
      expect(input).toHaveValue("gemma4:26b");
    });

    it("offers 再読み込み in the fallback and switches to the select once Ollama answers", async () => {
      let up = false;
      setupIpc({ get_settings: () => settings, ollama_models: () => { if (!up) throw { message: "refused" }; return ["gemma4:26b", "qwen3:8b"]; } });
      render(<LlmTab />);
      await screen.findByText("Ollama に接続できないため、モデル名を入力してください");
      up = true;
      await userEvent.click(ollamaSection().getByRole("button", { name: "再読み込み" }));
      await waitFor(() => expect(ollamaSection().getByLabelText("モデル").tagName).toBe("SELECT"));
      expect(ollamaSection().getByRole("option", { name: "qwen3:8b" })).toBeInTheDocument();
    });

    it("ignores a list that arrives after a newer request for a changed URL", async () => {
      let url = "http://localhost:11434";
      const slow = deferred<string[]>();
      let n = 0;
      setupIpc({
        get_settings: () => ({ ...settings, "ollama.base_url": url }),
        set_setting: (a) => { if (a.key === "ollama.base_url") url = a.value; },
        ollama_models: () => { n += 1; return n === 2 ? slow.promise : n === 1 ? ["first:1"] : ["new:1"]; },
      });
      render(<LlmTab />);
      await screen.findByRole("region", { name: "Ollama" });
      expect(await ollamaSection().findByRole("option", { name: "first:1" })).toBeInTheDocument();
      await userEvent.click(ollamaSection().getByRole("button", { name: "再読み込み" }));
      const urlField = ollamaSection().getByLabelText("URL");
      await userEvent.clear(urlField);
      await userEvent.type(urlField, "http://other:11434{enter}");
      expect(await ollamaSection().findByRole("option", { name: "new:1" })).toBeInTheDocument();
      await act(async () => { slow.resolve(["stale:1"]); await slow.promise; });
      expect(ollamaSection().queryByRole("option", { name: "stale:1" })).not.toBeInTheDocument();
      expect(ollamaSection().getByRole("option", { name: "new:1" })).toBeInTheDocument();
    });
  });

  describe("Claude model select", () => {
    it("offers sonnet, opus, fable and a custom entry, and saves claude.model", async () => {
      const ipc = setupIpc({ get_settings: () => settings, set_setting: () => undefined, ollama_models: () => [] });
      render(<LlmTab />);
      await screen.findByRole("region", { name: "Claude" });
      const select = claudeSection().getByLabelText("モデル");
      expect(select).toHaveValue("sonnet");
      expect(within(select).getAllByRole("option").map((o) => o.textContent)).toEqual(["sonnet", "opus", "fable", "その他 (名前を入力)"]);
      await userEvent.selectOptions(select, "opus");
      expect(ipc.last("set_setting")).toEqual({ key: "claude.model", value: "opus" });
    });

    it("shows a text input for a custom name and saves it", async () => {
      const ipc = setupIpc({ get_settings: () => settings, set_setting: () => undefined, ollama_models: () => [] });
      render(<LlmTab />);
      await screen.findByRole("region", { name: "Claude" });
      expect(claudeSection().queryByLabelText("モデル名")).not.toBeInTheDocument();
      await userEvent.selectOptions(claudeSection().getByLabelText("モデル"), "その他 (名前を入力)");
      const input = claudeSection().getByLabelText("モデル名");
      await userEvent.type(input, "claude-opus-4-1{enter}");
      expect(ipc.last("set_setting")).toEqual({ key: "claude.model", value: "claude-opus-4-1" });
    });

    it("rejects a custom name that could be read as a CLI flag", async () => {
      const ipc = setupIpc({ get_settings: () => settings, set_setting: () => undefined, ollama_models: () => [] });
      render(<LlmTab />);
      await screen.findByRole("region", { name: "Claude" });
      await userEvent.selectOptions(claudeSection().getByLabelText("モデル"), "その他 (名前を入力)");
      await userEvent.type(claudeSection().getByLabelText("モデル名"), "--dangerously-skip-permissions{enter}");
      expect(claudeSection().getByText(/英数字/)).toBeInTheDocument();
      expect(ipc.count("set_setting")).toBe(0);
      await userEvent.clear(claudeSection().getByLabelText("モデル名"));
      await userEvent.type(claudeSection().getByLabelText("モデル名"), "sonnet[[1m]{enter}");
      expect(ipc.last("set_setting")).toEqual({ key: "claude.model", value: "sonnet[1m]" });
    });

    it("shows a stored custom name in the text input", async () => {
      setupIpc({ get_settings: () => ({ ...settings, "claude.model": "claude-opus-4-1" }), ollama_models: () => [] });
      render(<LlmTab />);
      await screen.findByRole("region", { name: "Claude" });
      expect(claudeSection().getByLabelText("モデル")).toHaveValue("__other__");
      expect(claudeSection().getByLabelText("モデル名")).toHaveValue("claude-opus-4-1");
    });
  });

  it("runs connection tests and shows the result or the error", async () => {
    setupIpc({
      get_settings: () => settings,
      test_ollama: () => ({ ok: true, message: "接続済み (http://localhost:11434/api/tags)", millis: 800 }),
      test_claude: () => ({ ok: false, message: "claude: Not logged in", millis: 5000 }),
    });
    render(<LlmTab />);
    await findOllamaModel();
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

  it("keeps an edit made right after the field appears, before its mount effects have run", async () => {
    setupIpc({ get_settings: () => settings, set_setting: () => undefined });
    const { container } = render(<LlmTab />);
    // 欄が DOM に現れた直後 (コミット後、passive effect が走る前) に同期で入力する
    const typed = new Promise<void>((resolve) => {
      const mo = new MutationObserver(() => {
        const input = container.querySelector<HTMLInputElement>("#claude-bin");
        if (!input) return;
        mo.disconnect();
        fireEvent.change(input, { target: { value: "/nope/claude" } });
        resolve();
      });
      mo.observe(container, { childList: true, subtree: true });
    });
    await screen.findByLabelText("claude コマンドのパス");
    await typed;
    await act(async () => { await new Promise((r) => setTimeout(r, 20)); });
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
    const model = await findOllamaModel();
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
    const model = await findOllamaModel();
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
    const model = await findOllamaModel();
    fireEvent.change(model, { target: { value: "がんま" } });
    fireEvent.keyDown(model, { key: "Enter", isComposing: true });
    expect(ipc.count("set_setting")).toBe(0);
    // WKWebView は変換を確定する Enter を isComposing: false, keyCode: 229 で届けることがある
    fireEvent.keyDown(model, { key: "Enter", keyCode: 229 });
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
    await findOllamaModel();
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
    await findOllamaModel();
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
    expect(ollamaSection().getByLabelText("モデル")).toBeInTheDocument();
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
    await findOllamaModel();
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
