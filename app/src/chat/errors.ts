import type { ProviderId } from "./context";

const TITLES: Record<string, string> = {
  llm_unreachable: "Ollama に接続できません",
  model_missing: "モデルが見つかりません",
  auth: "Claude にログインしていません",
  tool: "データを読み出せません",
};
const NETWORK_TITLE = "エージェントに接続できません";
const NETWORK_PATTERN = /fetch|network|ECONNREFUSED|Load failed/i;

export interface ChatErrorView {
  kind: string;
  provider: ProviderId | null;
  title: string;
  message: string;
}

export function parseChatError(error: Error | undefined): ChatErrorView | null {
  if (!error) return null;
  try {
    const v = JSON.parse(error.message);
    if (v && typeof v.kind === "string" && typeof v.message === "string") {
      const provider = v.provider === "ollama" || v.provider === "claude" ? v.provider : null;
      return { kind: v.kind, provider, title: TITLES[v.kind] ?? "回答を受け取れませんでした", message: v.message };
    }
  } catch { /* JSON でなければ下へ */ }
  if (NETWORK_PATTERN.test(error.message)) {
    return { kind: "network", provider: null, title: NETWORK_TITLE, message: error.message };
  }
  return { kind: "other", provider: null, title: "回答を受け取れませんでした", message: error.message };
}
