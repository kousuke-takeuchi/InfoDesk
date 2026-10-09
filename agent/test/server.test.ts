import type { ServerResponse } from "node:http";
import type { AddressInfo } from "node:net";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it, vi } from "vitest";
import { simulateReadableStream } from "ai";
import { MockLanguageModelV4 } from "ai/test";
import { createCliRunner } from "../src/cli";
import { streamChat, type ChatDeps } from "../src/chat";
import { createAgentServer } from "../src/server";

const FAKE = fileURLToPath(new URL("./fake-infodesk.mjs", import.meta.url));
const TOKEN = "t0ken";
const ORIGIN = "tauri://localhost";
const usage = {
  inputTokens: { total: 1, noCache: 1, cacheRead: undefined, cacheWrite: undefined },
  outputTokens: { total: 1, text: 1, reasoning: undefined },
};

let server: ReturnType<typeof createAgentServer> | null = null;
afterEach(() => { server?.close(); server = null; });

type ChatFn = Parameters<typeof createAgentServer>[0]["chat"];

async function start(chat: ChatFn = vi.fn(async (_req: unknown, res: ServerResponse) => { res.end("ok"); })) {
  server = createAgentServer({ token: TOKEN, allowedOrigins: [ORIGIN], chat });
  await new Promise<void>((r) => server!.listen(0, "127.0.0.1", r));
  const { port } = server.address() as AddressInfo;
  return { base: `http://127.0.0.1:${port}`, chat };
}

const userMessage = { id: "u1", role: "user", parts: [{ type: "text", text: "今週の認知症の動きは?" }] };
const body = (extra: object = {}) => JSON.stringify({ messages: [userMessage], provider: "ollama", context: null, ...extra });

function sseChunks(text: string): any[] {
  return text.split("\n").filter((l) => l.startsWith("data: ") && l !== "data: [DONE]").map((l) => JSON.parse(l.slice(6)));
}

describe("createAgentServer", () => {
  it("requires the bearer token", async () => {
    const { base, chat } = await start();
    expect((await fetch(`${base}/health`)).status).toBe(401);
    expect((await fetch(`${base}/chat`, { method: "POST", body: body(), headers: { Authorization: "Bearer wrong" } })).status).toBe(401);
    expect(chat).not.toHaveBeenCalled();
    const ok = await fetch(`${base}/health`, { headers: { Authorization: `Bearer ${TOKEN}` } });
    expect(await ok.json()).toEqual({ ok: true });
  });

  it("answers CORS preflight only for allowed origins", async () => {
    const { base } = await start();
    const pre = await fetch(`${base}/chat`, { method: "OPTIONS", headers: { Origin: ORIGIN, "Access-Control-Request-Method": "POST" } });
    expect(pre.status).toBe(204);
    expect(pre.headers.get("access-control-allow-origin")).toBe(ORIGIN);
    expect(pre.headers.get("access-control-allow-headers")).toContain("authorization");
    const bad = await fetch(`${base}/chat`, { method: "OPTIONS", headers: { Origin: "https://evil.example" } });
    expect(bad.status).toBe(403);
  });

  it("validates the request body", async () => {
    const { base, chat } = await start();
    const h = { Authorization: `Bearer ${TOKEN}`, "Content-Type": "application/json" };
    expect((await fetch(`${base}/chat`, { method: "POST", headers: h, body: "{" })).status).toBe(400);
    expect((await fetch(`${base}/chat`, { method: "POST", headers: h, body: body({ provider: "gpt" }) })).status).toBe(400);
    expect((await fetch(`${base}/chat`, { method: "POST", headers: h, body: JSON.stringify({ provider: "ollama" }) })).status).toBe(400);
    expect((await fetch(`${base}/nope`, { headers: h })).status).toBe(404);
    expect(chat).not.toHaveBeenCalled();
  });
});

describe("streamChat (Ollama)", () => {
  function deps(model: MockLanguageModelV4): ChatDeps {
    return {
      run: createCliRunner([FAKE], "/tmp/x.db"),
      loadSettings: async () => ({ "ollama.base_url": "http://localhost:11434", "ollama.model": "gemma4:26b" }),
      models: { ollama: () => model, claude: () => { throw new Error("not used"); } },
      now: () => new Date("2026-10-09T10:00:00+09:00"),
    };
  }

  it("runs the tool loop and streams tool parts then text", async () => {
    let call = 0;
    const model = new MockLanguageModelV4({
      doStream: async () => {
        call += 1;
        const chunks = call === 1
          ? [
              { type: "tool-call", toolCallId: "c1", toolName: "search_articles", input: JSON.stringify({ query: "認知症" }) },
              { type: "finish", finishReason: { unified: "tool-calls", raw: undefined }, usage },
            ]
          : [
              { type: "text-start", id: "t" },
              { type: "text-delta", id: "t", delta: "新薬の話題が中心です。" },
              { type: "text-end", id: "t" },
              { type: "finish", finishReason: { unified: "stop", raw: undefined }, usage },
            ];
        return { stream: simulateReadableStream({ chunks: chunks as never }) };
      },
    });
    const d = deps(model);
    const { base } = await start((req, res, signal) => streamChat(req, res, d, signal));
    const res = await fetch(`${base}/chat`, { method: "POST", headers: { Authorization: `Bearer ${TOKEN}`, Origin: ORIGIN }, body: body() });
    expect(res.status).toBe(200);
    expect(res.headers.get("access-control-allow-origin")).toBe(ORIGIN);
    const chunks = sseChunks(await res.text());
    const types = chunks.map((c) => c.type);
    expect(types).toContain("tool-input-available");
    const out = chunks.find((c) => c.type === "tool-output-available");
    expect(out.output.argv).toContain("--text=認知症");
    expect(chunks.filter((c) => c.type === "text-delta").map((c) => c.delta).join("")).toBe("新薬の話題が中心です。");
    expect(types.at(-1)).toBe("finish");
    // 指示文が渡っている
    const firstPrompt = (model.doStreamCalls[0] as any).prompt;
    expect(JSON.stringify(firstPrompt)).toContain("InfoDesk");
  });

  it("streams an LLM failure as a structured error chunk", async () => {
    const model = new MockLanguageModelV4({
      doStream: async () => { throw Object.assign(new TypeError("fetch failed"), { cause: { code: "ECONNREFUSED" } }); },
    });
    const d = deps(model);
    const { base } = await start((req, res, signal) => streamChat(req, res, d, signal));
    const res = await fetch(`${base}/chat`, { method: "POST", headers: { Authorization: `Bearer ${TOKEN}` }, body: body() });
    const err = sseChunks(await res.text()).find((c) => c.type === "error");
    expect(JSON.parse(err.errorText)).toMatchObject({ kind: "llm_unreachable", provider: "ollama" });
  });

  it("returns a failing tool to the model instead of crashing", async () => {
    let call = 0;
    const model = new MockLanguageModelV4({
      doStream: async () => {
        call += 1;
        const chunks = call === 1
          ? [
              { type: "tool-call", toolCallId: "c1", toolName: "get_article", input: JSON.stringify({ id: 404 }) },
              { type: "finish", finishReason: { unified: "tool-calls", raw: undefined }, usage },
            ]
          : [
              { type: "text-start", id: "t" }, { type: "text-delta", id: "t", delta: "見つかりませんでした。" }, { type: "text-end", id: "t" },
              { type: "finish", finishReason: { unified: "stop", raw: undefined }, usage },
            ];
        return { stream: simulateReadableStream({ chunks: chunks as never }) };
      },
    });
    const d = deps(model);
    const { base } = await start((req, res, signal) => streamChat(req, res, d, signal));
    const res = await fetch(`${base}/chat`, { method: "POST", headers: { Authorization: `Bearer ${TOKEN}` }, body: body() });
    const chunks = sseChunks(await res.text());
    const toolErr = chunks.find((c) => c.type === "tool-output-error");
    expect(toolErr.errorText).toContain("item 404 not found");
    expect(chunks.some((c) => c.type === "text-delta")).toBe(true);
  });
});

describe("streamChat (Claude)", () => {
  it("bridges the tools over MCP and allows only them plus WebSearch", async () => {
    const claude = vi.fn(() => new MockLanguageModelV4({
      doStream: async () => ({
        stream: simulateReadableStream({ chunks: [
          { type: "text-start", id: "t" }, { type: "text-delta", id: "t", delta: "はい" }, { type: "text-end", id: "t" },
          { type: "finish", finishReason: { unified: "stop", raw: undefined }, usage },
        ] as never }),
      }),
    }));
    const d: ChatDeps = {
      run: createCliRunner([FAKE], "/tmp/x.db"),
      loadSettings: async () => ({ "claude.bin": "/usr/local/bin/claude", "ollama.web_search_api_key": "k" }),
      models: { ollama: () => { throw new Error("not used"); }, claude },
      now: () => new Date("2026-10-09T10:00:00+09:00"),
    };
    const { base } = await start((req, res, signal) => streamChat(req, res, d, signal));
    const res = await fetch(`${base}/chat`, { method: "POST", headers: { Authorization: `Bearer ${TOKEN}` }, body: body({ provider: "claude" }) });
    expect(sseChunks(await res.text()).some((c) => c.type === "text-delta")).toBe(true);
    const [settings, opts] = claude.mock.calls[0] as any;
    expect(settings["claude.bin"]).toBe("/usr/local/bin/claude");
    expect(Object.keys(opts.mcpServers)).toEqual(["infodesk"]);
    expect(opts.allowedTools).toEqual([
      "mcp__infodesk__search_articles", "mcp__infodesk__get_article", "mcp__infodesk__read_digests",
      "mcp__infodesk__fetch_page", "WebSearch",
    ]);
    expect(opts.instructions).toContain("InfoDesk");
  });
});
