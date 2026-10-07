import { describe, expect, it } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SourcesTab } from "./SourcesTab";
import { setupIpc } from "../../test/ipc";
import type { Source } from "../../types";

const sources: Source[] = [
  { id: 1, kind: "github_trending", name: "GitHub Trending (日)", category: "github", config: "daily", enabled: true, last_fetched_at: "2026-10-06T00:00:00Z", last_error: null },
  { id: 2, kind: "rss", name: "Zenn", category: "tech_jp", config: "https://zenn.dev/feed", enabled: true, last_fetched_at: "2026-10-06T00:00:00Z", last_error: "http: timeout" },
  { id: 3, kind: "rss", name: "The Verge", category: "tech_intl", config: "https://www.theverge.com/rss/index.xml", enabled: false, last_fetched_at: null, last_error: null },
];

describe("SourcesTab", () => {
  it("lists sources with state and toggles, deletes", async () => {
    let list = [...sources];
    const ipc = setupIpc({
      list_sources: () => list,
      set_source_enabled: (a) => { list = list.map((s) => (s.id === a.id ? { ...s, enabled: a.enabled } : s)); },
      delete_source: (a) => { list = list.filter((s) => s.id !== a.id); },
    });
    render(<SourcesTab />);
    expect(await screen.findByText("GitHub Trending (日)")).toBeInTheDocument();
    expect(screen.getByText("https://zenn.dev/feed")).toBeInTheDocument();
    expect(screen.getByText("http: timeout")).toBeInTheDocument();
    expect(screen.getByText("無効")).toBeInTheDocument();
    expect(screen.getByText(/3件のソース · 有効 2件 · エラー 1件/)).toBeInTheDocument();

    await userEvent.click(screen.getByRole("switch", { name: "The Verge を有効にする" }));
    expect(ipc.last("set_source_enabled")).toEqual({ id: 3, enabled: true });
    await waitFor(() => expect(screen.getByRole("switch", { name: /The Verge/ })).toHaveAttribute("aria-checked", "true"));

    await userEvent.click(screen.getByRole("button", { name: "Zenn を削除" }));
    expect(ipc.last("delete_source")).toEqual({ id: 2 });
    await waitFor(() => expect(screen.queryByText("Zenn")).not.toBeInTheDocument());
  });

  it("adds an RSS feed through the dialog and shows validation errors", async () => {
    // Review Focus 4
    let list = [...sources];
    const ipc = setupIpc({
      list_sources: () => list,
      add_rss_source: (a) => {
        if (a.url.includes("example.com")) throw { message: "invalid feed: no root element" };
        const s: Source = { id: 9, kind: "rss", name: a.name ?? "GIGAZINE", category: a.category, config: a.url, enabled: true, last_fetched_at: null, last_error: null };
        list = [...list, s];
        return s;
      },
    });
    render(<SourcesTab />);
    await screen.findByText("Zenn");
    await userEvent.click(screen.getByRole("button", { name: "RSSを追加" }));
    const dialog = screen.getByRole("dialog");
    const add = screen.getByRole("button", { name: "追加" });
    expect(add).toBeDisabled();

    await userEvent.type(screen.getByLabelText("フィードURL"), "https://example.com/");
    await userEvent.selectOptions(screen.getByLabelText("カテゴリー"), "tech_jp");
    expect(add).toBeEnabled();
    await userEvent.click(add);
    expect(await screen.findByText(/invalid feed: no root element/)).toBeInTheDocument();
    expect(dialog).toBeInTheDocument();
    expect(ipc.count("list_sources")).toBe(1);

    await userEvent.clear(screen.getByLabelText("フィードURL"));
    await userEvent.type(screen.getByLabelText("フィードURL"), "https://gigazine.net/news/rss_2.0/");
    await userEvent.click(screen.getByRole("button", { name: "追加" }));
    expect(ipc.last("add_rss_source")).toEqual({ url: "https://gigazine.net/news/rss_2.0/", name: null, category: "tech_jp" });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(await screen.findByText("GIGAZINE")).toBeInTheDocument();
  });

  it("edits a source inline", async () => {
    let list = [...sources];
    const ipc = setupIpc({
      list_sources: () => list,
      update_source: (a) => { list = list.map((s) => (s.id === a.id ? { ...s, name: a.name, category: a.category, config: a.config } : s)); },
    });
    render(<SourcesTab />);
    await screen.findByText("Zenn");
    await userEvent.click(screen.getByRole("button", { name: "Zenn を編集" }));
    const name = screen.getByLabelText("表示名");
    await userEvent.clear(name);
    await userEvent.type(name, "Zenn トレンド");
    await userEvent.click(screen.getByRole("button", { name: "保存" }));
    expect(ipc.last("update_source")).toEqual({ id: 2, name: "Zenn トレンド", category: "tech_jp", config: "https://zenn.dev/feed" });
    expect(await screen.findByText("Zenn トレンド")).toBeInTheDocument();
  });
});
