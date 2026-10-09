import type { ServerResponse } from "node:http";
import { convertToModelMessages, isStepCount, streamText, ToolLoopAgent, pipeAgentUIStreamToResponse, type UIMessage } from "ai";
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

const MAX_STEPS = 12;

export async function streamChat(req: ChatRequest, res: ServerResponse, deps: ChatDeps, signal: AbortSignal): Promise<void> {
  const settings = await deps.loadSettings();
  const onError = (e: unknown) => describeError(e, req.provider, settings);
  if (req.provider === "ollama") {
    const tools = createInfodeskTools(deps.run, { webSearchKey: settings["ollama.web_search_api_key"] || undefined });
    const agent = new ToolLoopAgent({
      model: deps.models.ollama(settings),
      instructions: buildInstructions(req.context, deps.now(), Object.keys(tools)),
      tools,
      stopWhen: isStepCount(MAX_STEPS),
    });
    await pipeAgentUIStreamToResponse({ response: res, agent, uiMessages: req.messages, abortSignal: signal, onError });
    return;
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
    messages: await convertToModelMessages(req.messages, { ignoreIncompleteToolCalls: true }),
    abortSignal: signal,
  });
  await result.pipeUIMessageStreamToResponse(res, { onError });
}
