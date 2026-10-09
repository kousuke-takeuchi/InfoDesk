import { describe, expect, it } from "vitest";
import { toolDetails } from "./toolDetails";

describe("toolDetails", () => {
  it("shows search conditions and up to 20 titles", () => {
    const items = Array.from({ length: 25 }, (_, i) => ({ id: i, title: `記事${i}` }));
    const rows = toolDetails("search_articles", { query: "認知症", category: "medcare", days: 7 }, { items, total: 25 });
    expect(rows[0]).toEqual({ label: "条件", value: "認知症 · 医療介護 · 直近7日" });
    expect(rows[1].label).toBe("記事 (25件)");
    expect(rows[1].value.split("\n")).toHaveLength(20);
  });

  it("shows an article, digests, a page and web results", () => {
    expect(toolDetails("get_article", { id: 1 }, { item: { title: "T", source: "S", summary: "要約" } }))
      .toEqual([{ label: "記事", value: "T" }, { label: "ソース", value: "S" }, { label: "要約", value: "要約" }]);
    expect(toolDetails("read_digests", { period: "week" }, { period: "week", period_start: "2026-10-05", digests: [{ label: "医療介護", status: "done" }, { label: "GitHub", status: "none" }] }))
      .toEqual([{ label: "期間", value: "週次 (2026-10-05〜)" }, { label: "要約", value: "医療介護: 生成済み\nGitHub: なし" }]);
    const page = toolDetails("mcp__infodesk__fetch_page", { url: "https://e.com/a" }, { page: { title: "P", text: "あ".repeat(400), truncated: true } });
    expect(page[0]).toEqual({ label: "ページ", value: "P" });
    expect(page[1].value.length).toBe(301);
    expect(page[2]).toEqual({ label: "切り詰め", value: "あり" });
    expect(toolDetails("web_search", { query: "q" }, { results: [{ title: "R", url: "https://r.com" }] }))
      .toEqual([{ label: "検索語", value: "q" }, { label: "結果", value: "R · https://r.com" }]);
  });

  it("opens MCP text results", () => {
    const wrapped = { content: [{ type: "text", text: JSON.stringify({ item: { title: "T" } }) }] };
    expect(toolDetails("mcp__infodesk__get_article", { id: 1 }, wrapped)).toEqual([{ label: "記事", value: "T" }]);
  });

  it("shows the error text for a failed tool", () => {
    expect(toolDetails("get_article", { id: 404 }, undefined, "item 404 not found")).toEqual([{ label: "エラー", value: "item 404 not found" }]);
  });
});
