import http, { type IncomingMessage, type ServerResponse } from "node:http";
import type { ChatRequest } from "./chat";

const MAX_BODY = 5 * 1024 * 1024;
const PROVIDERS = new Set(["ollama", "claude"]);

export interface AgentServerOptions {
  token: string;
  allowedOrigins: string[];
  chat: (req: ChatRequest, res: ServerResponse, signal: AbortSignal) => Promise<void>;
}

function send(res: ServerResponse, status: number, body?: unknown) {
  res.statusCode = status;
  if (body === undefined) return res.end();
  res.setHeader("Content-Type", "application/json");
  res.end(JSON.stringify(body));
}

async function readBody(req: IncomingMessage): Promise<string | null> {
  let size = 0;
  const chunks: Buffer[] = [];
  for await (const c of req) {
    size += (c as Buffer).length;
    if (size > MAX_BODY) return null;
    chunks.push(c as Buffer);
  }
  return Buffer.concat(chunks).toString("utf8");
}

function parseChat(raw: string): ChatRequest | null {
  try {
    const v = JSON.parse(raw);
    if (!v || !Array.isArray(v.messages) || !PROVIDERS.has(v.provider)) return null;
    return { messages: v.messages, provider: v.provider, context: v.context ?? null };
  } catch {
    return null;
  }
}

export function createAgentServer(opts: AgentServerOptions): http.Server {
  return http.createServer(async (req, res) => {
    const origin = req.headers.origin;
    if (origin) {
      if (!opts.allowedOrigins.includes(origin)) return send(res, 403);
      res.setHeader("Access-Control-Allow-Origin", origin);
      res.setHeader("Vary", "Origin");
    }
    if (req.method === "OPTIONS") {
      res.setHeader("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
      res.setHeader("Access-Control-Allow-Headers", "authorization, content-type");
      res.setHeader("Access-Control-Max-Age", "600");
      return send(res, 204);
    }
    if (req.headers.authorization !== `Bearer ${opts.token}`) return send(res, 401, { error: "unauthorized" });
    const url = new URL(req.url ?? "/", "http://127.0.0.1");
    if (req.method === "GET" && url.pathname === "/health") return send(res, 200, { ok: true });
    if (req.method !== "POST" || url.pathname !== "/chat") return send(res, 404, { error: "not found" });
    const raw = await readBody(req);
    if (raw === null) return send(res, 413, { error: "request too large" });
    const chat = parseChat(raw);
    if (!chat) return send(res, 400, { error: "invalid chat request" });
    const ac = new AbortController();
    res.on("close", () => { if (!res.writableFinished) ac.abort(); });
    try {
      await opts.chat(chat, res, ac.signal);
    } catch (e) {
      if (!res.headersSent) send(res, 500, { error: e instanceof Error ? e.message : String(e) });
      else res.end();
    }
  });
}
