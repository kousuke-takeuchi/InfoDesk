import type { LanguageModel } from "ai";
import { createOllama } from "ai-sdk-ollama";
import { claudeCode } from "ai-sdk-provider-claude-code";

export type ProviderId = "ollama" | "claude";
export type Settings = Record<string, string>;

/** `claude.model` が空のときに使う Claude のモデル。InfoDesk の他の設定と同じく CLI 経由で使う。 */
export const CLAUDE_DEFAULT_MODEL = "sonnet";

/** Ollama のチャットの文脈長。core の要約 (gemma4:26b) と同じ値。既定のままだと長いツール結果や会話が黙って切れる。 */
export const OLLAMA_NUM_CTX = 16384;

/** Claude の1回の回答で使えるターン数の上限。Ollama の MAX_STEPS と同じ。 */
export const CLAUDE_MAX_TURNS = 20;

export interface ClaudeModelOptions {
  instructions: string;
  mcpServers: Record<string, unknown>;
  allowedTools: string[];
}

export interface ModelFactory {
  ollama(settings: Settings): LanguageModel;
  claude(settings: Settings, opts: ClaudeModelOptions): LanguageModel;
}

export const defaultModels: ModelFactory = {
  ollama: (s) =>
    createOllama({ baseURL: s["ollama.base_url"] || "http://localhost:11434" })(s["ollama.model"] || "gemma4:26b", {
      options: { num_ctx: OLLAMA_NUM_CTX },
    }),
  claude: (s, o) =>
    claudeCode(s["claude.model"]?.trim() || CLAUDE_DEFAULT_MODEL, {
      pathToClaudeCodeExecutable: s["claude.bin"] || "claude",
      systemPrompt: o.instructions,
      mcpServers: o.mcpServers as never,
      tools: ["WebSearch"],
      allowedTools: o.allowedTools,
      persistSession: false,
      maxTurns: CLAUDE_MAX_TURNS,
    }),
};
