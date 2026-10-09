import { CATEGORIES } from "../types";
import { obj, stripMcpPrefix } from "./unwrap";

const PERIOD = { day: "日次", week: "週次", month: "月次" } as Record<string, string>;
const LABELS: Record<string, string> = {
  search_articles: "記事検索",
  get_article: "記事を読む",
  read_digests: "要約を読む",
  fetch_page: "本文取得",
  web_search: "Web検索",
  WebSearch: "Web検索",
};

export function describeTool(toolName: string, args: unknown, result: unknown): { label: string; detail: string } {
  const name = stripMcpPrefix(toolName);
  const label = LABELS[name];
  if (!label) return { label: toolName, detail: "" };
  const a = obj(args);
  const r = obj(result);
  const parts: string[] = [];
  switch (name) {
    case "search_articles":
      if (a.query) parts.push(String(a.query));
      if (a.category) parts.push(CATEGORIES.find((c) => c.id === a.category)?.label ?? String(a.category));
      if (a.topic) parts.push(String(a.topic));
      if (a.days) parts.push(`直近${a.days}日`);
      if (typeof r.total === "number") parts.push(`${r.total}件`);
      break;
    case "get_article":
      parts.push(obj(r.item).title ? String(obj(r.item).title) : `id ${a.id ?? "?"}`);
      break;
    case "fetch_page":
      if (obj(r.page).title) parts.push(String(obj(r.page).title));
      else {
        try { parts.push(new URL(String(a.url)).host); } catch { /* URL でなければ何も出さない */ }
      }
      break;
    case "read_digests":
      parts.push(PERIOD[String(a.period)] ?? "");
      break;
    default:
      if (a.query) parts.push(String(a.query));
  }
  return { label, detail: parts.filter(Boolean).join(" · ") };
}
