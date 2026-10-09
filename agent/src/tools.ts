import { tool, type Tool } from "ai";
import { z } from "zod";
import type { CliRunner } from "./cli";

export const TOOL_NAMES = ["search_articles", "get_article", "read_digests", "fetch_page", "update_plan", "web_search"] as const;

const CATEGORIES = ["github", "arxiv", "huggingface", "hackernews", "tech_jp", "tech_intl", "medcare"] as const;
const DATE = z.string().regex(/^\d{4}-\d{2}-\d{2}$/, "YYYY-MM-DD");
const WEB_SEARCH_URL = "https://ollama.com/api/web_search";
const WEB_SNIPPET_CHARS = 1000;
const PAGE_MAX_CHARS = 20000;
const ONE_SCOPE = "category と topic はどちらか一方だけ指定する";

function flag(args: string[], name: string, value: string | number | undefined) {
  if (value !== undefined && value !== "") args.push(`${name}=${String(value)}`);
}

export function createInfodeskTools(
  run: CliRunner,
  opts: { webSearchKey?: string; fetchImpl?: typeof fetch; pageMaxChars?: number },
): Record<string, Tool> {
  const pageMaxChars = opts.pageMaxChars ?? PAGE_MAX_CHARS;
  const tools: Record<string, Tool> = {
    search_articles: tool({
      description:
        "収集済みの記事を検索する。キーワード、カテゴリー、トピック名、取得日の範囲で絞り込める。新しい順に返す。",
      inputSchema: z.object({
        query: z.string().min(1).max(200).optional().describe("タイトルと概要に含まれる語"),
        category: z.enum(CATEGORIES).optional(),
        topic: z.string().min(1).optional().describe("トピック名 (例: 認知症)"),
        days: z.number().int().min(1).max(365).optional().describe("直近N日に取得した記事"),
        from: DATE.optional().describe("この日以降に取得した記事"),
        to: DATE.optional().describe("この日までに取得した記事"),
        limit: z.number().int().min(1).max(50).optional(),
      }),
      execute: async (input, { abortSignal }) => {
        const args = ["items", "--json"];
        flag(args, "--text", input.query);
        flag(args, "--category", input.category);
        flag(args, "--topic", input.topic);
        flag(args, "--days", input.days);
        flag(args, "--from", input.from);
        flag(args, "--to", input.to);
        flag(args, "--limit", input.limit ?? 20);
        return run(args, abortSignal);
      },
    }),
    get_article: tool({
      description: "収集済みの記事1件の詳細 (概要、記事要約、URL) を読む。",
      inputSchema: z.object({ id: z.number().int().positive() }),
      execute: async ({ id }, { abortSignal }) => run(["item", String(id), "--json"], abortSignal),
    }),
    read_digests: tool({
      description:
        "生成済みの期間要約 (日次、週次、月次) を読む。生成はしない。category と topic はどちらか一方だけ指定する。両方省くと全カテゴリー。",
      inputSchema: z
        .object({
          period: z.enum(["day", "week", "month"]),
          date: DATE.optional().describe("期間に含まれる日。省くと今日"),
          category: z.enum(CATEGORIES).optional(),
          topic: z.string().min(1).optional(),
        })
        .refine((v) => v.category === undefined || v.topic === undefined, { message: ONE_SCOPE }),
      execute: async (input, { abortSignal }) => {
        // Claude の MCP 経路はスキーマの shape だけを使い refine が効かないので、ここでも確かめる
        if (input.category !== undefined && input.topic !== undefined) throw new Error(ONE_SCOPE);
        const args = ["digests", input.period, "--json"];
        flag(args, "--date", input.date);
        flag(args, "--category", input.category);
        flag(args, "--topic", input.topic);
        return run(args, abortSignal);
      },
    }),
    fetch_page: tool({
      description: "記事の URL を取得して本文のテキストを返す。長い本文は切り詰める。保存はしない。",
      inputSchema: z.object({
        url: z.url({ protocol: /^https?$/ }),
        max_chars: z.number().int().min(1000).max(PAGE_MAX_CHARS).optional(),
      }),
      execute: async ({ url, max_chars }, { abortSignal }) =>
        run(["page", url, "--json", `--max-chars=${max_chars ?? pageMaxChars}`], abortSignal),
    }),
    update_plan: tool({
      description:
        "作業計画を示す、または更新する。ツールを2回以上使う質問で最初に呼び、進むたびに各項目の状態を更新する。",
      inputSchema: z.object({
        items: z.array(z.object({
          title: z.string().min(1).max(80),
          status: z.enum(["pending", "in_progress", "done"]),
        })).min(1).max(8),
      }),
      execute: async ({ items }) => ({ ok: true, count: items.length }),
    }),
  };
  if (opts.webSearchKey) {
    const key = opts.webSearchKey;
    const doFetch = opts.fetchImpl ?? fetch;
    tools.web_search = tool({
      description: "収集した記事の外を Web で検索する。収集済みの記事で足りないときだけ使う。",
      inputSchema: z.object({
        query: z.string().min(1).max(500),
        max_results: z.number().int().min(1).max(10).optional(),
      }),
      execute: async ({ query, max_results }, { abortSignal }) => {
        const res = await doFetch(WEB_SEARCH_URL, {
          method: "POST",
          headers: { "Content-Type": "application/json", Authorization: `Bearer ${key}` },
          body: JSON.stringify({ query }),
          signal: abortSignal,
        });
        if (!res.ok) throw new Error(`web search failed: HTTP ${res.status}`);
        const data = (await res.json()) as { results?: Array<Record<string, unknown>> };
        const results = (data.results ?? []).slice(0, max_results ?? 5).map((r) => {
          const content = String(r.content ?? r.snippet ?? "");
          return {
            title: String(r.title ?? ""),
            url: String(r.url ?? ""),
            content: content.length > WEB_SNIPPET_CHARS ? `${content.slice(0, WEB_SNIPPET_CHARS)}…` : content,
          };
        });
        return { query, results };
      },
    });
  }
  return tools;
}
