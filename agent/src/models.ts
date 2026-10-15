import type { LanguageModel } from "ai";
import { createOllama } from "ai-sdk-ollama";
import { claudeCode } from "ai-sdk-provider-claude-code";

export type ProviderId = "ollama" | "claude";
export type Settings = Record<string, string>;

/** `claude.model` が空のときに使う Claude のモデル。InfoDesk の他の設定と同じく CLI 経由で使う。 */
export const CLAUDE_DEFAULT_MODEL = "sonnet";

/** core の is_valid_model_name と同じ規則。CLI の引数がフラグとして読まれないよう先頭の "-" を拒む。 */
const MODEL_NAME = /^[A-Za-z0-9._:[\]][A-Za-z0-9._:[\]-]*$/;

/** `claude.model` から使うモデル名を決める。空なら既定、使えない名前なら例外。 */
export function claudeModelName(s: Settings): string {
  const name = s["claude.model"]?.trim() || CLAUDE_DEFAULT_MODEL;
  if (!MODEL_NAME.test(name)) throw new Error(`claude.model: invalid model name '${name}'`);
  return name;
}

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
    claudeCode(claudeModelName(s), {
      pathToClaudeCodeExecutable: s["claude.bin"] || "claude",
      systemPrompt: o.instructions,
      mcpServers: o.mcpServers as never,
      tools: ["WebSearch"],
      allowedTools: o.allowedTools,
      persistSession: false,
      maxTurns: CLAUDE_MAX_TURNS,
    }),
};
