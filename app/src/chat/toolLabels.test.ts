import { describe, expect, it } from "vitest";
import { describeTool } from "./toolLabels";

describe("describeTool", () => {
  it("labels the infodesk tools with or without the MCP prefix", () => {
    expect(describeTool("search_articles", { query: "認知症", days: 7 }, { items: [], total: 14 }))
      .toEqual({ label: "記事検索", detail: "認知症 · 直近7日 · 14件" });
    expect(describeTool("mcp__infodesk__search_articles", { category: "medcare" }, undefined))
      .toEqual({ label: "記事検索", detail: "医療介護" });
    expect(describeTool("get_article", { id: 12 }, { item: { id: 12, title: "新薬の記事" } }))
      .toEqual({ label: "記事を読む", detail: "新薬の記事" });
    expect(describeTool("fetch_page", { url: "https://www.example.com/a/b" }, { page: { title: "介護報酬改定の論点整理" } }))
      .toEqual({ label: "本文取得", detail: "介護報酬改定の論点整理" });
    expect(describeTool("fetch_page", { url: "https://www.example.com/a/b" }, undefined))
      .toEqual({ label: "本文取得", detail: "www.example.com" });
    expect(describeTool("read_digests", { period: "week" }, undefined)).toEqual({ label: "要約を読む", detail: "週次" });
    expect(describeTool("web_search", { query: "認知症 新薬" }, undefined)).toEqual({ label: "Web検索", detail: "認知症 新薬" });
    expect(describeTool("WebSearch", { query: "x" }, undefined)).toEqual({ label: "Web検索", detail: "x" });
  });

  it("falls back to the raw name", () => {
    expect(describeTool("mcp__other__thing", {}, undefined)).toEqual({ label: "mcp__other__thing", detail: "" });
  });

  it("does not treat plan updates as a labelled tool", () => {
    expect(describeTool("update_plan", { items: [] }, undefined)).toEqual({ label: "update_plan", detail: "" });
  });
});
