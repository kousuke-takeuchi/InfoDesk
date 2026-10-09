import type { ServerResponse } from "node:http";
import {
  convertToModelMessages, createAgentUIStream, createUIMessageStream, isStepCount, pipeUIMessageStreamToResponse,
  streamText, ToolLoopAgent, type UIMessage, type UIMessageChunk,
} from "ai";
import { createAiSdkMcpServer } from "ai-sdk-provider-claude-code";
import type { CliRunner } from "./cli";
import type { ChatContext } from "./context";
import { describeError } from "./errors";
import type { ModelFactory, ProviderId, Settings } from "./models";
import { buildInstructions } from "./prompt";
import { createInfodeskTools } from "./tools";

export interface ChatRequest {
  messages: UIMessage[];
  provider: ProviderId;
  context: ChatContext | null;
}

export interface ChatDeps {
  run: CliRunner;
  loadSettings(): Promise<Settings>;
  models: ModelFactory;
  now(): Date;
}

/** Ollama の1回の回答で使えるステップ数の上限。作業計画の更新の分も見込む。 */
export const MAX_STEPS = 20;
const FINISHED_TOOL_STATES = new Set(["output-available", "output-error", "output-denied"]);
/** Ollama では fetch_page の既定の長さを抑える (文脈長に収めるため)。入力の上限は Claude と同じ。 */
export const OLLAMA_PAGE_MAX_CHARS = 6000;
/** Ollama に送る履歴で、最新より前のアシスタントのツール結果を JSON でこの文字数までに縮める。 */
export const OLDER_TOOL_OUTPUT_CHARS = 2000;

function isToolPart(p: unknown): boolean {
  const type = (p as { type?: unknown }).type;
  return typeof type === "string" && (type.startsWith("tool-") || type === "dynamic-tool");
}

/**
 * 途中で止めた回の会話には、結果のないツール呼び出しが残る。そのままだと以後の回がすべて失敗するので、
 * 終わっていないツール部分を落とし、空になったアシスタントのメッセージも落とす。
 */
export function dropIncompleteToolParts(messages: UIMessage[]): UIMessage[] {
  return messages.flatMap((m) => {
    if (!Array.isArray(m.parts)) return [m];
    const parts = m.parts.filter(
      (p) => !isToolPart(p) || FINISHED_TOOL_STATES.has(String((p as { state?: unknown }).state)),
    );
    if (m.role === "assistant" && parts.length === 0) return [];
    return [{ ...m, parts }];
  });
}

/**
 * Ollama の文脈長に収めるため、最新のアシスタントのメッセージより前のツール結果のうち、
 * JSON が長いものを先頭だけの要約に置き換える。最新の回はそのまま残す。入力は変更しない。
 */
export function truncateOlderToolOutputs(messages: UIMessage[], maxChars = OLDER_TOOL_OUTPUT_CHARS): UIMessage[] {
  let latest = -1;
  messages.forEach((m, i) => {
    if (m.role === "assistant") latest = i;
  });
  return messages.map((m, i) => {
    if (m.role !== "assistant" || i === latest || !Array.isArray(m.parts)) return m;
    let changed = false;
    const parts = m.parts.map((p) => {
      if (!isToolPart(p) || !("output" in p)) return p;
      const json = JSON.stringify((p as { output?: unknown }).output) ?? "";
      if (json.length <= maxChars) return p;
      changed = true;
      return { ...p, output: { truncated: true, preview: json.slice(0, maxChars) } };
    });
    return changed ? ({ ...m, parts } as UIMessage) : m;
  });
}

/**
 * 接続が切れたとき、ai の UI ストリームを理由付きで cancel すると Node の pipeTo 内部で未処理の reject が出る。
 * 中断は abort シグナルで伝わるので、cancel は伝えずに残りを読み捨てる (シグナルの中断ですぐ終わる)。
 */
function drainOnCancel(inner: ReadableStream<UIMessageChunk>): ReadableStream<UIMessageChunk> {
  const reader = inner.getReader();
  return new ReadableStream<UIMessageChunk>({
    async pull(controller) {
      const { done, value } = await reader.read();
      if (done) controller.close();
      else controller.enqueue(value);
    },
    cancel() {
      void (async () => {
        try {
          while (!(await reader.read()).done);
        } catch {
          // 読み捨てるだけなので無視する
        }
      })();
    },
  });
}

function metadataFor(deps: ChatDeps) {
  return ({ part }: { part: { type: string } }) =>
    part.type === "start" ? { startedAt: deps.now().toISOString() }
    : part.type === "finish" ? { finishedAt: deps.now().toISOString() }
    : undefined;
}

async function openStream(req: ChatRequest, deps: ChatDeps, settings: Settings, signal: AbortSignal): Promise<ReadableStream<UIMessageChunk>> {
  const onError = (e: unknown) => describeError(e, req.provider, settings);
  const messages = dropIncompleteToolParts(req.messages);
  if (req.provider === "ollama") {
    const tools = createInfodeskTools(deps.run, {
      webSearchKey: settings["ollama.web_search_api_key"] || undefined,
      pageMaxChars: OLLAMA_PAGE_MAX_CHARS,
    });
    const agent = new ToolLoopAgent({
      model: deps.models.ollama(settings),
      instructions: buildInstructions(req.context, deps.now(), Object.keys(tools)),
      tools,
      stopWhen: isStepCount(MAX_STEPS),
    });
    return createAgentUIStream({ agent, uiMessages: truncateOlderToolOutputs(messages), abortSignal: signal, onError, messageMetadata: metadataFor(deps), sendReasoning: true }) as Promise<ReadableStream<UIMessageChunk>>;
  }
  // Claude: Claude Code CLI が自分でツールを実行するので、ツールは MCP で渡す。Web検索は組み込みの WebSearch。
  const tools = createInfodeskTools(deps.run, {});
  const names = Object.keys(tools);
  const instructions = buildInstructions(req.context, deps.now(), [...names, "web_search"]);
  const model = deps.models.claude(settings, {
    instructions,
    mcpServers: { infodesk: createAiSdkMcpServer("infodesk", tools as never) },
    allowedTools: [...names.map((n) => `mcp__infodesk__${n}`), "WebSearch"],
  });
  const result = streamText({
    model,
    messages: await convertToModelMessages(messages, { ignoreIncompleteToolCalls: true }),
    abortSignal: signal,
  });
  return result.toUIMessageStream({ onError, messageMetadata: metadataFor(deps), sendReasoning: true });
}

/** ストリームを始める前の失敗 (設定の読み込み、モデルの用意、メッセージの検証) も、UI には error チャンクで返す。 */
async function sendErrorStream(res: ServerResponse, errorText: string): Promise<void> {
  const stream = createUIMessageStream({
    execute: ({ writer }) => {
      writer.write({ type: "start" });
      writer.write({ type: "error", errorText });
    },
  });
  await pipeUIMessageStreamToResponse({ response: res, stream });
}

export async function streamChat(req: ChatRequest, res: ServerResponse, deps: ChatDeps, signal: AbortSignal): Promise<void> {
  let settings: Settings | undefined;
  let stream: ReadableStream<UIMessageChunk>;
  try {
    settings = await deps.loadSettings();
    stream = await openStream(req, deps, settings, signal);
  } catch (e) {
    await sendErrorStream(res, describeError(e, req.provider, settings ?? {}));
    return;
  }
  await pipeUIMessageStreamToResponse({ response: res, stream: drainOnCancel(stream) });
}
