import { fileURLToPath } from "node:url";
import { describe, expect, it, vi } from "vitest";
import { createCliRunner } from "../src/cli";
import { createInfodeskTools, TOOL_NAMES } from "../src/tools";

const FAKE = fileURLToPath(new URL("./fake-infodesk.mjs", import.meta.url));
const run = createCliRunner([FAKE], "/tmp/x.db");
const opts = { toolCallId: "t1", messages: [] } as never;

async function exec(tools: Record<string, any>, name: string, input: unknown) {
  return tools[name].execute(input, opts);
}

describe("createInfodeskTools", () => {
  it("offers web_search only when a key is set", () => {
    expect(Object.keys(createInfodeskTools(run, {}))).toEqual([
      "search_articles", "get_article", "read_digests", "fetch_page", "update_plan",
    ]);
    expect(Object.keys(createInfodeskTools(run, { webSearchKey: "k" }))).toContain("web_search");
    expect(TOOL_NAMES).toEqual(["search_articles", "get_article", "read_digests", "fetch_page", "update_plan", "web_search"]);
  });

  it("accepts a plan update and validates it", async () => {
    const tools = createInfodeskTools(run, {});
    const plan = { items: [{ title: "記事を検索", status: "done" }, { title: "まとめる", status: "in_progress" }] };
    expect(await exec(tools, "update_plan", plan)).toEqual({ ok: true, count: 2 });
    const schema = (tools.update_plan as any).inputSchema;
    expect(schema.safeParse({ items: [] }).success).toBe(false);
    expect(schema.safeParse({ items: Array.from({ length: 9 }, () => ({ title: "x", status: "pending" })) }).success).toBe(false);
    expect(schema.safeParse({ items: [{ title: "x", status: "doing" }] }).success).toBe(false);
    expect(TOOL_NAMES).toContain("update_plan");
  });

  it("maps search_articles input to CLI flags", async () => {
    const out = await exec(createInfodeskTools(run, {}), "search_articles", {
      query: "認知症", category: "medcare", topic: "介護", from: "2026-10-01", to: "2026-10-07", limit: 5,
    });
    expect(out).toEqual({
      argv: ["--db", "/tmp/x.db", "items", "--json", "--text=認知症", "--category=medcare",
        "--topic=介護", "--from=2026-10-01", "--to=2026-10-07", "--limit=5"],
    });
    const plain = await exec(createInfodeskTools(run, {}), "search_articles", {});
    expect(plain).toEqual({ argv: ["--db", "/tmp/x.db", "items", "--json", "--limit=20"] });
  });

  it("maps the other tools to their commands", async () => {
    const tools = createInfodeskTools(run, {});
    expect(await exec(tools, "get_article", { id: 12 })).toEqual({ argv: ["--db", "/tmp/x.db", "item", "12", "--json"] });
    expect(await exec(tools, "read_digests", { period: "week", date: "2026-10-07", topic: "認知症" })).toEqual({
      argv: ["--db", "/tmp/x.db", "digests", "week", "--json", "--date=2026-10-07", "--topic=認知症"],
    });
    expect(await exec(tools, "fetch_page", { url: "https://example.com/a" })).toEqual({
      argv: ["--db", "/tmp/x.db", "page", "https://example.com/a", "--json", "--max-chars=20000"],
    });
  });

  it("uses the page size default it is given for fetch_page", async () => {
    expect(await exec(createInfodeskTools(run, { pageMaxChars: 6000 }), "fetch_page", { url: "https://example.com/a" })).toEqual({
      argv: ["--db", "/tmp/x.db", "page", "https://example.com/a", "--json", "--max-chars=6000"],
    });
    expect(await exec(createInfodeskTools(run, { pageMaxChars: 6000 }), "fetch_page", { url: "https://example.com/a", max_chars: 20000 })).toEqual({
      argv: ["--db", "/tmp/x.db", "page", "https://example.com/a", "--json", "--max-chars=20000"],
    });
  });

  it("rejects read_digests with both category and topic", () => {
    const tool = createInfodeskTools(run, {}).read_digests;
    const schema = tool.inputSchema as unknown as {
      safeParse(v: unknown): { success: boolean; error?: { issues: Array<{ message: string }> } };
    };
    const both = schema.safeParse({ period: "day", category: "medcare", topic: "認知症" });
    expect(both.success).toBe(false);
    expect(both.error?.issues[0].message).toBe("category と topic はどちらか一方だけ指定する");
    expect(schema.safeParse({ period: "day", category: "medcare" }).success).toBe(true);
    expect(schema.safeParse({ period: "day", topic: "認知症" }).success).toBe(true);
    expect(tool.description).toContain("category と topic はどちらか一方だけ");
  });

  it("also rejects both in execute, since the Claude MCP bridge keeps only the object shape", async () => {
    await expect(
      exec(createInfodeskTools(run, {}), "read_digests", { period: "day", category: "medcare", topic: "認知症" }),
    ).rejects.toThrow("category と topic はどちらか一方だけ指定する");
  });

  it("passes values that start with a dash as part of the flag", async () => {
    const out = await exec(createInfodeskTools(run, {}), "search_articles", { query: "--json" });
    expect(out.argv).toContain("--text=--json");
  });

  it("accepts only http and https URLs for fetch_page", () => {
    const schema = createInfodeskTools(run, {}).fetch_page.inputSchema as unknown as {
      safeParse(v: unknown): { success: boolean };
    };
    expect(schema.safeParse({ url: "file:///etc/passwd" }).success).toBe(false);
    expect(schema.safeParse({ url: "ftp://example.com/a" }).success).toBe(false);
    expect(schema.safeParse({ url: "https://example.com/a" }).success).toBe(true);
    expect(schema.safeParse({ url: "http://example.com/a" }).success).toBe(true);
  });

  it("returns a tool failure as an error the model can read", async () => {
    const tools = createInfodeskTools(run, {});
    await expect(exec(tools, "get_article", { id: 404 })).rejects.toThrow("item 404 not found");
  });

  it("calls ollama.com web search with the key and trims results", async () => {
    const fetchImpl = vi.fn(async () =>
      new Response(JSON.stringify({ results: [
        { title: "A", url: "https://a", content: "x".repeat(2000) },
        { title: "B", url: "https://b", content: "b" },
        { url: "https://c" },
      ] }), { status: 200 }),
    );
    const tools = createInfodeskTools(run, { webSearchKey: "secret", fetchImpl: fetchImpl as never });
    const out = await exec(tools, "web_search", { query: "認知症 新薬", max_results: 2 });
    const [url, init] = fetchImpl.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe("https://ollama.com/api/web_search");
    expect(init.method).toBe("POST");
    expect((init.headers as Record<string, string>).Authorization).toBe("Bearer secret");
    expect(JSON.parse(String(init.body))).toEqual({ query: "認知症 新薬" });
    expect(out.results).toHaveLength(2);
    expect(out.results[0].content.length).toBe(1001);
  });

  it("reports web search HTTP errors without the key", async () => {
    const fetchImpl = vi.fn(async () => new Response("unauthorized", { status: 401 }));
    const tools = createInfodeskTools(run, { webSearchKey: "secret", fetchImpl: fetchImpl as never });
    const err = await exec(tools, "web_search", { query: "x" }).catch((e: Error) => e);
    expect(err.message).toBe("web search failed: HTTP 401");
    expect(err.message).not.toContain("secret");
  });
});
