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
      "search_articles", "get_article", "read_digests", "fetch_page",
    ]);
    expect(Object.keys(createInfodeskTools(run, { webSearchKey: "k" }))).toContain("web_search");
    expect(TOOL_NAMES).toEqual(["search_articles", "get_article", "read_digests", "fetch_page", "web_search"]);
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
