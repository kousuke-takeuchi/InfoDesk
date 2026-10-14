import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SettingsView } from "./SettingsView";
import { setupIpc } from "../test/ipc";
import type { AppStatus } from "../types";

const status: AppStatus = { collecting: false, summarizing: false, digesting: false, last_collect_at: null, next_collect_at: null, last_error: null };
const settings = {
  "llm.items_provider": "ollama", "llm.digest_provider": "claude", "llm.chat_provider": "ollama",
  "ollama.base_url": "http://localhost:11434", "ollama.model": "gemma4:26b", "ollama.web_search_api_key": "",
  "claude.bin": "claude", "node.bin": "node", "summary.top_n": "20",
};

describe("SettingsView", () => {
  it("opens on the sources tab by default", async () => {
    setupIpc({ list_sources: () => [], source_item_counts: () => ({}) });
    render(<SettingsView status={status} />);
    expect(await screen.findByRole("button", { name: "RSSを追加" })).toBeInTheDocument();
  });

  it("opens on the tab given by initialTab", async () => {
    const ipc = setupIpc({ get_settings: () => settings });
    render(<SettingsView status={status} initialTab="llm" />);
    expect(await screen.findByLabelText("モデル")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "LLM" })).toHaveAttribute("aria-pressed", "true");
    expect(ipc.unmocked).toEqual([]);
  });

  it("has the sync tab last", async () => {
    setupIpc({ list_sources: () => [], source_item_counts: () => ({}), sync_status: () => ({ available: true, signed_in: false, account_email: null, enabled: false, syncing: false, last_sync_at: null, error: null, device_name: "Mac" }) });
    render(<SettingsView status={status} />);
    const tabs = screen.getByRole("group", { name: "設定のタブ" });
    expect(Array.from(tabs.querySelectorAll("button")).map((b) => b.textContent)).toEqual(["ソース", "トピック", "LLM", "スケジュール", "同期"]);
    await userEvent.click(screen.getByRole("button", { name: "同期" }));
    expect(await screen.findByRole("button", { name: "Google でログイン" })).toBeInTheDocument();
  });
});
