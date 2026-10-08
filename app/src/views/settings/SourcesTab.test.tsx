import { describe, expect, it } from "vitest";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SourcesTab } from "./SourcesTab";
import { setupIpc } from "../../test/ipc";
import type { Source } from "../../types";

const sources: Source[] = [
  { id: 1, kind: "github_trending", name: "GitHub Trending (日)", category: "github", config: "daily", enabled: true, last_fetched_at: "2026-10-06T00:00:00Z", last_error: null },
  { id: 2, kind: "rss", name: "Zenn", category: "tech_jp", config: "https://zenn.dev/feed", enabled: true, last_fetched_at: "2026-10-06T00:00:00Z", last_error: "http: timeout" },
  { id: 3, kind: "rss", name: "The Verge", category: "tech_intl", config: "https://www.theverge.com/rss/index.xml", enabled: false, last_fetched_at: null, last_error: null },
];

const rowOf = (name: string) => within(screen.getByText(name).closest(".table-row") as HTMLElement);

describe("SourcesTab", () => {
  it("lists sources with state and toggles, deletes", async () => {
    let list = [...sources];
    const ipc = setupIpc({
      list_sources: () => list,
      source_item_counts: () => ({}),
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
    expect(ipc.count("delete_source")).toBe(0);
    await userEvent.click(screen.getByRole("button", { name: "削除する" }));
    expect(ipc.last("delete_source")).toEqual({ id: 2 });
    await waitFor(() => expect(screen.queryByText("Zenn")).not.toBeInTheDocument());
  });

  it("adds an RSS feed through the dialog and shows validation errors", async () => {
    // Review Focus 4
    let list = [...sources];
    const ipc = setupIpc({
      list_sources: () => list,
      source_item_counts: () => ({}),
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
      source_item_counts: () => ({}),
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

  it("renders the edit card immediately after the row being edited", async () => {
    setupIpc({ list_sources: () => sources, source_item_counts: () => ({}) });
    render(<SourcesTab />);
    await screen.findByText("Zenn");
    const rowEl = (name: string) => screen.getByText(name).closest(".table-row") as HTMLElement;
    const card = () => screen.getByText("ソースを編集").closest(".card");

    await userEvent.click(screen.getByRole("button", { name: "GitHub Trending (日) を編集" }));
    expect(rowEl("GitHub Trending (日)").nextElementSibling).toBe(card());
    expect(card()?.nextElementSibling).toBe(rowEl("Zenn"));

    await userEvent.click(screen.getByRole("button", { name: "Zenn を編集" }));
    expect(screen.getAllByText("ソースを編集")).toHaveLength(1);
    expect(rowEl("Zenn").nextElementSibling).toBe(card());
    expect(card()?.nextElementSibling).toBe(rowEl("The Verge"));
  });

  it("shows the editor of the row clicked last and saves that row", async () => {
    // F1: 編集カードを開いたまま別の行の鉛筆を押す
    let list = [...sources];
    const ipc = setupIpc({
      list_sources: () => list,
      source_item_counts: () => ({}),
      update_source: (a) => { list = list.map((s) => (s.id === a.id ? { ...s, name: a.name, category: a.category, config: a.config } : s)); },
    });
    render(<SourcesTab />);
    await screen.findByText("Zenn");
    await userEvent.click(screen.getByRole("button", { name: "Zenn を編集" }));
    expect(screen.getByLabelText("表示名")).toHaveValue("Zenn");
    await userEvent.click(screen.getByRole("button", { name: "The Verge を編集" }));
    expect(screen.getByLabelText("表示名")).toHaveValue("The Verge");
    expect(screen.getByLabelText("カテゴリー")).toHaveValue("tech_intl");
    await userEvent.click(screen.getByRole("button", { name: "保存" }));
    expect(ipc.last("update_source")).toEqual({ id: 3, name: "The Verge", category: "tech_intl", config: "https://www.theverge.com/rss/index.xml" });
    expect(ipc.count("update_source")).toBe(1);
  });

  it("asks before deleting, one row at a time", async () => {
    // F2
    let list = [...sources];
    const ipc = setupIpc({
      list_sources: () => list,
      source_item_counts: () => ({}),
      delete_source: (a) => { list = list.filter((s) => s.id !== a.id); },
    });
    render(<SourcesTab />);
    await screen.findByText("Zenn");

    await userEvent.click(screen.getByRole("button", { name: "Zenn を削除" }));
    expect(rowOf("Zenn").getByText("削除しますか?")).toBeInTheDocument();
    expect(rowOf("Zenn").queryByRole("button", { name: "Zenn を編集" })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "キャンセル" }));
    expect(screen.queryByText("削除しますか?")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Zenn を削除" })).toBeInTheDocument();
    expect(ipc.count("delete_source")).toBe(0);

    await userEvent.click(screen.getByRole("button", { name: "Zenn を削除" }));
    await userEvent.click(screen.getByRole("button", { name: "The Verge を削除" }));
    expect(screen.getAllByText("削除しますか?")).toHaveLength(1);
    expect(rowOf("The Verge").getByText("削除しますか?")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Zenn を削除" })).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "削除する" }));
    expect(ipc.last("delete_source")).toEqual({ id: 3 });
    expect(ipc.count("delete_source")).toBe(1);
    await waitFor(() => expect(screen.queryByText("The Verge")).not.toBeInTheDocument());
    expect(screen.getByText("Zenn")).toBeInTheDocument();
    expect(screen.queryByText("削除しますか?")).not.toBeInTheDocument();
  });

  it("closes the edit card when the edited source is deleted", async () => {
    let list = [...sources];
    setupIpc({
      list_sources: () => list,
      source_item_counts: () => ({}),
      delete_source: (a) => { list = list.filter((s) => s.id !== a.id); },
    });
    render(<SourcesTab />);
    await screen.findByText("Zenn");
    await userEvent.click(screen.getByRole("button", { name: "GitHub Trending (日) を編集" }));
    await userEvent.click(screen.getByRole("button", { name: "Zenn を削除" }));
    await userEvent.click(screen.getByRole("button", { name: "削除する" }));
    await waitFor(() => expect(screen.queryByText("Zenn")).not.toBeInTheDocument());
    expect(screen.getByLabelText("表示名")).toHaveValue("GitHub Trending (日)");

    await userEvent.click(screen.getByRole("button", { name: "GitHub Trending (日) を削除" }));
    await userEvent.click(screen.getByRole("button", { name: "削除する" }));
    await waitFor(() => expect(screen.queryByLabelText("表示名")).not.toBeInTheDocument());
    expect(screen.queryByText("ソースを編集")).not.toBeInTheDocument();
  });

  it("shows config read-only in the edit card and sends it unchanged", async () => {
    // F3
    let list = [...sources];
    const ipc = setupIpc({
      list_sources: () => list,
      source_item_counts: () => ({}),
      update_source: (a) => { list = list.map((s) => (s.id === a.id ? { ...s, name: a.name, category: a.category, config: a.config } : s)); },
    });
    render(<SourcesTab />);
    await screen.findByText("Zenn");
    await userEvent.click(screen.getByRole("button", { name: "Zenn を編集" }));
    const card = within(screen.getByText("ソースを編集").closest(".card") as HTMLElement);
    expect(card.getAllByRole("textbox")).toHaveLength(1);
    expect(card.getByRole("textbox")).toBe(screen.getByLabelText("表示名"));
    expect(screen.queryByLabelText("フィードURL")).not.toBeInTheDocument();
    expect(card.getByText("フィードURL")).toBeInTheDocument();
    expect(card.getByText("https://zenn.dev/feed")).toBeInTheDocument();
    expect(card.getByText("URLを変える場合は、削除してから追加し直してください。")).toBeInTheDocument();

    await userEvent.selectOptions(screen.getByLabelText("カテゴリー"), "medcare");
    await userEvent.click(screen.getByRole("button", { name: "保存" }));
    expect(ipc.last("update_source")).toEqual({ id: 2, name: "Zenn", category: "medcare", config: "https://zenn.dev/feed" });

    await waitFor(() => expect(screen.queryByText("ソースを編集")).not.toBeInTheDocument());
    await userEvent.click(screen.getByRole("button", { name: "GitHub Trending (日) を編集" }));
    const other = within(screen.getByText("ソースを編集").closest(".card") as HTMLElement);
    expect(other.getByText("設定値")).toBeInTheDocument();
    expect(other.getByText("daily")).toBeInTheDocument();
    expect(other.queryByText(/URLを変える場合/)).not.toBeInTheDocument();
    expect(other.getAllByRole("textbox")).toHaveLength(1);
  });

  it("sends one update when 保存 is clicked twice", async () => {
    let finish: () => void = () => {};
    const ipc = setupIpc({
      list_sources: () => sources,
      source_item_counts: () => ({}),
      update_source: () => new Promise<void>((r) => { finish = () => r(); }),
    });
    render(<SourcesTab />);
    await screen.findByText("Zenn");
    await userEvent.click(screen.getByRole("button", { name: "Zenn を編集" }));
    const save = screen.getByRole("button", { name: "保存" });
    await userEvent.click(save);
    expect(save).toBeDisabled();
    await userEvent.click(save);
    expect(ipc.count("update_source")).toBe(1);
    await act(async () => { finish(); });
    await waitFor(() => expect(screen.queryByText("ソースを編集")).not.toBeInTheDocument());
    expect(ipc.count("update_source")).toBe(1);
  });

  it("shows item counts, errors, disabled, and never-fetched in the state column", async () => {
    // F5
    const list: Source[] = [
      ...sources,
      { id: 4, kind: "rss", name: "Publickey", category: "tech_jp", config: "https://www.publickey1.jp/atom.xml", enabled: true, last_fetched_at: null, last_error: null },
      { id: 5, kind: "rss", name: "GIGAZINE", category: "tech_jp", config: "https://gigazine.net/news/rss_2.0/", enabled: true, last_fetched_at: "2026-10-06T00:00:00Z", last_error: null },
    ];
    const ipc = setupIpc({
      list_sources: () => list,
      source_item_counts: () => ({ "1": 25, "2": 40, "3": 12 }),
    });
    render(<SourcesTab />);
    await screen.findByText("Zenn");
    expect(ipc.count("source_item_counts")).toBe(1);

    const ok = rowOf("GitHub Trending (日)");
    expect(ok.getByText("25件")).toBeInTheDocument();
    expect(ok.getByText("25件").querySelector(".dot-success")).not.toBeNull();

    const failed = rowOf("Zenn");
    expect(failed.getByText("http: timeout")).toBeInTheDocument();
    // エラー文は折り返して見せ (1行に切り詰めない)、全文は title にも持つ
    expect(failed.getByText("http: timeout")).toHaveClass("clamp-3");
    expect(failed.getByText("http: timeout")).not.toHaveClass("truncate");
    expect(failed.getByTitle("http: timeout")).toHaveClass("cell-status", "error");
    expect(failed.queryByText("40件")).not.toBeInTheDocument();

    const disabled = rowOf("The Verge");
    expect(disabled.getByText("無効")).toBeInTheDocument();
    expect(disabled.queryByText("12件")).not.toBeInTheDocument();

    const never = rowOf("Publickey");
    expect(never.getByText("未取得")).toBeInTheDocument();
    expect(never.queryByText("0件")).not.toBeInTheDocument();

    expect(rowOf("GIGAZINE").getByText("0件")).toBeInTheDocument();
    expect(screen.getByText(/5件のソース · 有効 4件 · エラー 1件/)).toBeInTheDocument();
    expect(ipc.unmocked).toEqual([]);
  });

  it("keeps the dialog open while the feed is being validated", async () => {
    // F4
    let list = [...sources];
    let finish: () => void = () => {};
    const ipc = setupIpc({
      list_sources: () => list,
      source_item_counts: () => ({}),
      add_rss_source: (a) => new Promise<Source>((r) => {
        finish = () => {
          const s: Source = { id: 9, kind: "rss", name: "GIGAZINE", category: a.category, config: a.url, enabled: true, last_fetched_at: null, last_error: null };
          list = [...list, s];
          r(s);
        };
      }),
    });
    render(<SourcesTab />);
    await screen.findByText("Zenn");
    await userEvent.click(screen.getByRole("button", { name: "RSSを追加" }));
    const dialog = screen.getByRole("dialog");
    const overlay = dialog.parentElement as HTMLElement;
    expect(overlay).toHaveClass("overlay");
    await userEvent.type(screen.getByLabelText("フィードURL"), "https://gigazine.net/news/rss_2.0/");
    await userEvent.click(screen.getByRole("button", { name: "追加" }));
    expect(await screen.findByText("フィードを取得して検証しています…")).toBeInTheDocument();

    const cancel = within(dialog).getByRole("button", { name: "キャンセル" });
    expect(cancel).toBeDisabled();
    await userEvent.click(cancel);
    await userEvent.click(overlay);
    await userEvent.keyboard("{Escape}");
    await userEvent.type(screen.getByLabelText("フィードURL"), "{Enter}");
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(ipc.count("add_rss_source")).toBe(1);

    await act(async () => { finish(); });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(await screen.findByText("GIGAZINE")).toBeInTheDocument();
  });

  it("blocks the Enter that confirms an IME composition in the dialog's text fields", async () => {
    const ipc = setupIpc({ list_sources: () => sources, source_item_counts: () => ({}) });
    render(<SourcesTab />);
    await screen.findByText("Zenn");
    await userEvent.click(screen.getByRole("button", { name: "RSSを追加" }));
    // fireEvent は既定の動作が止められたとき false を返す
    for (const label of ["フィードURL", "表示名 (任意)"]) {
      const field = screen.getByLabelText(label);
      fireEvent.change(field, { target: { value: "https://example.com/ふぃーど" } });
      expect(fireEvent.keyDown(field, { key: "Enter", isComposing: true })).toBe(false);
      // WKWebView は変換を確定する Enter を isComposing: false, keyCode: 229 で届けることがある
      expect(fireEvent.keyDown(field, { key: "Enter", keyCode: 229 })).toBe(false);
      expect(fireEvent.keyDown(field, { key: "Enter", keyCode: 13 })).toBe(true);
      expect(fireEvent.keyDown(field, { key: "a", keyCode: 229 })).toBe(true);
    }
    expect(ipc.count("add_rss_source")).toBe(0);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("focuses the URL field, submits on Enter, and closes on Escape or overlay click when idle", async () => {
    let list = [...sources];
    const ipc = setupIpc({
      list_sources: () => list,
      source_item_counts: () => ({}),
      add_rss_source: (a) => {
        const s: Source = { id: 9, kind: "rss", name: "GIGAZINE", category: a.category, config: a.url, enabled: true, last_fetched_at: null, last_error: null };
        list = [...list, s];
        return s;
      },
    });
    render(<SourcesTab />);
    await screen.findByText("Zenn");

    await userEvent.click(screen.getByRole("button", { name: "RSSを追加" }));
    expect(screen.getByLabelText("フィードURL")).toHaveFocus();
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "RSSを追加" }));
    await userEvent.click(screen.getByRole("dialog").parentElement as HTMLElement);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "RSSを追加" }));
    await userEvent.keyboard("{Enter}");
    expect(ipc.count("add_rss_source")).toBe(0);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    await userEvent.keyboard("https://gigazine.net/news/rss_2.0/{Enter}");
    expect(ipc.last("add_rss_source")).toEqual({ url: "https://gigazine.net/news/rss_2.0/", name: null, category: "tech_jp" });
    expect(ipc.count("add_rss_source")).toBe(1);
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(await screen.findByText("GIGAZINE")).toBeInTheDocument();
  });
});
