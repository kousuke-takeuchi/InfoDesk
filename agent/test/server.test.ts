import type { ServerResponse } from "node:http";
import net, { type AddressInfo } from "node:net";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it, vi } from "vitest";
import { simulateReadableStream } from "ai";
import { MockLanguageModelV4 } from "ai/test";
import { CliError, createCliRunner } from "../src/cli";
import { MAX_STEPS, streamChat, type ChatDeps, type ChatRequest } from "../src/chat";
import { createAgentServer } from "../src/server";

const FAKE = fileURLToPath(new URL("./fake-infodesk.mjs", import.meta.url));
const TOKEN = "t0ken";
const ORIGIN = "tauri://localhost";
const NOW = new Date("2026-10-09T10:00:00+09:00");
const NOW_ISO = NOW.toISOString();
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
    expect((await fetch(`${base}/health`, { headers: { Authorization: "Bearer t0kem" } })).status).toBe(401);
    expect((await fetch(`${base}/health`, { headers: { Authorization: `Bearer ${TOKEN}x` } })).status).toBe(401);
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

  it("rejects a body over the limit with 413", async () => {
    const { base, chat } = await start();
    const big = JSON.stringify({ messages: [{ id: "u", role: "user", parts: [{ type: "text", text: "a".repeat(6 * 1024 * 1024) }] }], provider: "ollama" });
    const res = await fetch(`${base}/chat`, { method: "POST", headers: { Authorization: `Bearer ${TOKEN}` }, body: big });
    expect(res.status).toBe(413);
    expect(chat).not.toHaveBeenCalled();
  });

  it("treats a malformed context as no context", async () => {
    const { base, chat } = await start();
    const h = { Authorization: `Bearer ${TOKEN}` };
    for (const context of ["x", { scope: { kind: "all" } }, { attachments: [] }, { scope: { kind: "all" }, attachments: "no" }]) {
      const res = await fetch(`${base}/chat`, { method: "POST", headers: h, body: body({ context }) });
      expect(res.status).toBe(200);
    }
    const good = { scope: { kind: "all" }, period: null, attachments: [] };
    await fetch(`${base}/chat`, { method: "POST", headers: h, body: body({ context: good }) });
    const contexts = (chat as any).mock.calls.map((c: [ChatRequest]) => c[0].context);
    expect(contexts).toEqual([null, null, null, null, good]);
  });

  it("survives a client that aborts during the upload", async () => {
    const { base } = await start();
    const port = Number(new URL(base).port);
    await new Promise<void>((resolve) => {
      const sock = net.connect(port, "127.0.0.1", () => {
        sock.write(`POST /chat HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer ${TOKEN}\r\nContent-Length: 1000\r\n\r\n{"messages":`);
        setTimeout(() => { sock.destroy(); resolve(); }, 50);
      });
    });
    await new Promise((r) => setTimeout(r, 100));
    const ok = await fetch(`${base}/health`, { headers: { Authorization: `Bearer ${TOKEN}` } });
    expect(await ok.json()).toEqual({ ok: true });
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

  it("sends start and finish times as message metadata", async () => {
    const model = new MockLanguageModelV4({
      doStream: async () => ({ stream: simulateReadableStream({ chunks: [
        { type: "text-start", id: "t" }, { type: "text-delta", id: "t", delta: "はい" }, { type: "text-end", id: "t" },
        { type: "finish", finishReason: { unified: "stop", raw: undefined }, usage },
      ] as never }) }),
    });
    const d = deps(model);
    const { base } = await start((req, res, signal) => streamChat(req, res, d, signal));
    const res = await fetch(`${base}/chat`, { method: "POST", headers: { Authorization: `Bearer ${TOKEN}` }, body: body() });
    const chunks = sseChunks(await res.text());
    expect(chunks.find((c) => c.type === "start").messageMetadata).toEqual({ startedAt: NOW_ISO });
    expect(chunks.find((c) => c.type === "finish").messageMetadata).toEqual({ finishedAt: NOW_ISO });
  });

  it("answers after plan updates sent alongside the tool calls, within the step budget", async () => {
    const plan = (statuses: string[]) => JSON.stringify({
      items: statuses.map((status, i) => ({ title: `手順${i + 1}`, status })),
    });
    const steps: object[][] = [
      [{ type: "tool-call", toolCallId: "p0", toolName: "update_plan", input: plan(["in_progress", "pending", "pending"]) }],
      ...[0, 1, 2].map((n) => [
        { type: "tool-call", toolCallId: `s${n}`, toolName: "search_articles", input: JSON.stringify({ query: `q${n}` }) },
        { type: "tool-call", toolCallId: `p${n + 1}`, toolName: "update_plan",
          input: plan(["done", "done", "done"].map((d, i) => (i < n ? d : i === n ? "in_progress" : "pending"))) },
      ]),
    ];
    let call = 0;
    const model = new MockLanguageModelV4({
      doStream: async () => {
        const step = steps[call];
        call += 1;
        const chunks = step
          ? [...step, { type: "finish", finishReason: { unified: "tool-calls", raw: undefined }, usage }]
          : [
              { type: "text-start", id: "t" }, { type: "text-delta", id: "t", delta: "まとめです。" }, { type: "text-end", id: "t" },
              { type: "finish", finishReason: { unified: "stop", raw: undefined }, usage },
            ];
        return { stream: simulateReadableStream({ chunks: chunks as never }) };
      },
    });
    const d = deps(model);
    const { base } = await start((req, res, signal) => streamChat(req, res, d, signal));
    const res = await fetch(`${base}/chat`, { method: "POST", headers: { Authorization: `Bearer ${TOKEN}` }, body: body() });
    const chunks = sseChunks(await res.text());
    expect(chunks.find((c) => c.type === "error")).toBeUndefined();
    expect(chunks.filter((c) => c.type === "tool-output-available" && c.output?.ok === true).length).toBe(4);
    expect(chunks.filter((c) => c.type === "text-delta").map((c) => c.delta).join("")).toBe("まとめです。");
    expect(model.doStreamCalls.length).toBe(5);
    expect(model.doStreamCalls.length).toBeLessThanOrEqual(MAX_STEPS);
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

  const textModel = (text: string) => new MockLanguageModelV4({
    doStream: async () => ({
      stream: simulateReadableStream({ chunks: [
        { type: "text-start", id: "t" }, { type: "text-delta", id: "t", delta: text }, { type: "text-end", id: "t" },
        { type: "finish", finishReason: { unified: "stop", raw: undefined }, usage },
      ] as never }),
    }),
  });

  it("streams a CLI failure before streaming as a structured tool error", async () => {
    const d: ChatDeps = { ...deps(textModel("x")), loadSettings: async () => { throw new CliError("infodesk CLI not found: /nope"); } };
    const { base } = await start((req, res, signal) => streamChat(req, res, d, signal));
    const res = await fetch(`${base}/chat`, { method: "POST", headers: { Authorization: `Bearer ${TOKEN}` }, body: body() });
    expect(res.status).toBe(200);
    expect(res.headers.get("content-type")).toContain("text/event-stream");
    const chunks = sseChunks(await res.text());
    expect(chunks.map((c) => c.type)).toEqual(["start", "error"]);
    expect(JSON.parse(chunks[1].errorText)).toEqual({
      kind: "tool", provider: "ollama", message: "infodesk CLI を実行できません: infodesk CLI not found: /nope",
    });
  });

  it("streams a model factory failure as a structured error", async () => {
    const d: ChatDeps = { ...deps(textModel("x")), models: { ollama: () => { throw new Error("bad base url"); }, claude: () => { throw new Error("no"); } } };
    const { base } = await start((req, res, signal) => streamChat(req, res, d, signal));
    const res = await fetch(`${base}/chat`, { method: "POST", headers: { Authorization: `Bearer ${TOKEN}` }, body: body() });
    expect(res.status).toBe(200);
    const err = sseChunks(await res.text()).find((c) => c.type === "error");
    expect(JSON.parse(err.errorText)).toEqual({ kind: "other", provider: "ollama", message: "bad base url" });
  });

  it("answers after a previous turn was aborted mid tool call", async () => {
    const model = textModel("続きです。");
    const d = deps(model);
    const { base } = await start((req, res, signal) => streamChat(req, res, d, signal));
    const messages = [
      userMessage,
      { id: "a1", role: "assistant", parts: [
        { type: "step-start" },
        { type: "tool-search_articles", toolCallId: "c0", state: "input-available", input: { query: "認知症" } },
      ] },
      { id: "a2", role: "assistant", parts: [
        { type: "text", text: "途中まで", state: "done" },
        { type: "dynamic-tool", toolName: "x", toolCallId: "c9", state: "input-streaming", input: undefined },
      ] },
      { id: "u2", role: "user", parts: [{ type: "text", text: "続けて" }] },
    ];
    const res = await fetch(`${base}/chat`, { method: "POST", headers: { Authorization: `Bearer ${TOKEN}` }, body: JSON.stringify({ messages, provider: "ollama", context: null }) });
    const chunks = sseChunks(await res.text());
    expect(chunks.find((c) => c.type === "error")).toBeUndefined();
    expect(chunks.filter((c) => c.type === "text-delta").map((c) => c.delta).join("")).toBe("続きです。");
    const prompt = JSON.stringify((model.doStreamCalls[0] as any).prompt);
    expect(prompt).not.toContain("c0");
    expect(prompt).toContain("途中まで");
  });

  it("aborts the chat when the client disconnects mid-stream", async () => {
    const model = new MockLanguageModelV4({
      doStream: async () => ({
        stream: simulateReadableStream({
          initialDelayInMs: 0,
          chunkDelayInMs: 200,
          chunks: [
            { type: "text-start", id: "t" },
            ...Array.from({ length: 20 }, () => ({ type: "text-delta", id: "t", delta: "あ" })),
            { type: "text-end", id: "t" },
            { type: "finish", finishReason: { unified: "stop", raw: undefined }, usage },
          ] as never,
        }),
      }),
    });
    const d = deps(model);
    let seen: AbortSignal | undefined;
    const { base } = await start((req, res, signal) => { seen = signal; return streamChat(req, res, d, signal); });
    const ac = new AbortController();
    const res = await fetch(`${base}/chat`, { method: "POST", headers: { Authorization: `Bearer ${TOKEN}` }, body: body(), signal: ac.signal });
    const reader = res.body!.getReader();
    await reader.read();
    ac.abort();
    await vi.waitFor(() => expect(seen?.aborted).toBe(true), { timeout: 2000 });
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
      "mcp__infodesk__fetch_page", "mcp__infodesk__update_plan", "WebSearch",
    ]);
    expect(opts.instructions).toContain("InfoDesk");
  });

  it("sends start and finish times as message metadata", async () => {
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
      loadSettings: async () => ({}),
      models: { ollama: () => { throw new Error("not used"); }, claude },
      now: () => NOW,
    };
    const { base } = await start((req, res, signal) => streamChat(req, res, d, signal));
    const res = await fetch(`${base}/chat`, { method: "POST", headers: { Authorization: `Bearer ${TOKEN}` }, body: body({ provider: "claude" }) });
    const chunks = sseChunks(await res.text());
    expect(chunks.find((c) => c.type === "start").messageMetadata).toEqual({ startedAt: NOW_ISO });
    expect(chunks.find((c) => c.type === "finish").messageMetadata).toEqual({ finishedAt: NOW_ISO });
  });
});
