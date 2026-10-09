import { isAuthenticationError } from "ai-sdk-provider-claude-code";
import { CliError } from "./cli";
import type { ProviderId, Settings } from "./models";

export interface ChatErrorInfo {
  kind: "llm_unreachable" | "model_missing" | "auth" | "tool" | "other";
  provider: ProviderId;
  message: string;
}

function causeCode(e: unknown): string | undefined {
  let cur: unknown = e;
  for (let i = 0; i < 4 && cur && typeof cur === "object"; i++) {
    const code = (cur as { code?: unknown }).code;
    if (typeof code === "string") return code;
    cur = (cur as { cause?: unknown }).cause;
  }
  return undefined;
}

export function describeError(error: unknown, provider: ProviderId, settings: Settings): string {
  const message = error instanceof Error ? error.message : String(error);
  const info = ((): ChatErrorInfo => {
    if (error instanceof CliError) {
      return { kind: "tool", provider, message: `infodesk CLI を実行できません: ${message}` };
    }
    if (provider === "ollama") {
      const code = causeCode(error);
      if (code === "ECONNREFUSED" || code === "ENOTFOUND" || /fetch failed|ECONNREFUSED/.test(message)) {
        const url = settings["ollama.base_url"] ?? "http://localhost:11434";
        return { kind: "llm_unreachable", provider, message: `${url} が応答しません。Ollama が起動しているか確認してください。` };
      }
      if (/model .* not found|not found, try pulling/i.test(message)) {
        const model = settings["ollama.model"] ?? "";
        return { kind: "model_missing", provider, message: `モデル ${model} が Ollama にありません。ollama pull ${model} を実行するか、設定でモデル名を直してください。` };
      }
    }
    if (provider === "claude" && (isAuthenticationError(error) || /\bauthentication\b|\blog ?in\b|\bunauthorized\b/i.test(message))) {
      return { kind: "auth", provider, message: "Claude にログインしていません。ターミナルで claude を実行してログインしてください。" };
    }
    return { kind: "other", provider, message };
  })();
  return JSON.stringify(info);
}
