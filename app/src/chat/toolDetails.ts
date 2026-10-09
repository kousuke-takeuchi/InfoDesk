import { CATEGORIES } from "../types";
import { obj, stripMcpPrefix, unwrap, type Obj } from "./unwrap";

type Row = { label: string; value: string };

const PERIOD: Record<string, string> = { day: "日次", week: "週次", month: "月次" };
const DIGEST_STATUS: Record<string, string> = { done: "生成済み", pending: "保留", failed: "失敗", none: "なし" };
const MAX_TITLES = 20;
const PAGE_HEAD = 300;

// Joins the present parts only, so a missing field never prints "undefined".
function joinPresent(head: unknown[], sep: string, tail: unknown): string {
  return [...head, tail].filter((p) => p != null && p !== "").map(String).join(sep);
}

export function toolDetails(toolName: string, args: unknown, result: unknown, errorText?: string): Row[] {
  if (errorText) return [{ label: "エラー", value: errorText }];
  const name = stripMcpPrefix(toolName);
  const a = obj(args);
  const r = unwrap(result) ?? {};
  const rows: Row[] = [];
  switch (name) {
    case "search_articles": {
      const cond: string[] = [];
      if (a.query) cond.push(String(a.query));
      if (a.category) cond.push(CATEGORIES.find((c) => c.id === a.category)?.label ?? String(a.category));
      if (a.topic) cond.push(String(a.topic));
      if (a.days) cond.push(`直近${a.days}日`);
      if (cond.length) rows.push({ label: "条件", value: cond.join(" · ") });
      const items: Obj[] = Array.isArray(r.items) ? r.items : [];
      if (items.length) {
        const total = typeof r.total === "number" ? r.total : items.length;
        rows.push({ label: `記事 (${total}件)`, value: items.slice(0, MAX_TITLES).map((i) => String(i?.title ?? "")).join("\n") });
      }
      break;
    }
    case "get_article": {
      const it = obj(r.item);
      if (it.title) rows.push({ label: "記事", value: String(it.title) });
      if (it.source) rows.push({ label: "ソース", value: String(it.source) });
      if (it.summary) rows.push({ label: "要約", value: String(it.summary) });
      break;
    }
    case "read_digests": {
      const period = PERIOD[String(r.period ?? a.period)];
      if (period) rows.push({ label: "期間", value: r.period_start ? `${period} (${r.period_start}〜)` : period });
      const digests: Obj[] = Array.isArray(r.digests) ? r.digests : [];
      if (digests.length) {
        const lines = digests.map((d) => {
          const status = d?.status == null ? "" : DIGEST_STATUS[String(d.status)] ?? String(d.status);
          return joinPresent([d?.label], ": ", status);
        });
        const value = lines.filter(Boolean).join("\n");
        if (value) rows.push({ label: "要約", value });
      }
      break;
    }
    case "fetch_page": {
      const page = obj(r.page);
      if (page.title) rows.push({ label: "ページ", value: String(page.title) });
      if (typeof page.text === "string" && page.text) {
        rows.push({ label: "冒頭", value: page.text.length > PAGE_HEAD ? `${page.text.slice(0, PAGE_HEAD)}…` : page.text });
      }
      if (page.truncated) rows.push({ label: "切り詰め", value: "あり" });
      break;
    }
    case "web_search":
    case "WebSearch": {
      if (a.query) rows.push({ label: "検索語", value: String(a.query) });
      const results: Obj[] = Array.isArray(r.results) ? r.results : [];
      const value = results.map((x) => joinPresent([x?.title], " · ", x?.url)).filter(Boolean).join("\n");
      if (value) rows.push({ label: "結果", value });
      break;
    }
  }
  return rows;
}
