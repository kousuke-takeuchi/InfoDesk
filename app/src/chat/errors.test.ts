import { describe, expect, it } from "vitest";
import { parseChatError } from "./errors";

describe("parseChatError", () => {
  it("reads the structured error from the sidecar", () => {
    const e = new Error(JSON.stringify({ kind: "llm_unreachable", provider: "ollama", message: "http://localhost:11434 が応答しません。" }));
    expect(parseChatError(e)).toEqual({ kind: "llm_unreachable", provider: "ollama", title: "Ollama に接続できません", message: "http://localhost:11434 が応答しません。" });
    const auth = new Error(JSON.stringify({ kind: "auth", provider: "claude", message: "ログインしていません。" }));
    expect(parseChatError(auth)?.title).toBe("Claude にログインしていません");
    const model = new Error(JSON.stringify({ kind: "model_missing", provider: "ollama", message: "m" }));
    expect(parseChatError(model)?.title).toBe("モデルが見つかりません");
    const tool = new Error(JSON.stringify({ kind: "tool", provider: "claude", message: "CLI failed" }));
    expect(parseChatError(tool)).toEqual({ kind: "tool", provider: "claude", title: "データを読み出せません", message: "CLI failed" });
  });

  it("treats anything else as a generic error", () => {
    expect(parseChatError(new Error("boom"))).toEqual({ kind: "other", provider: null, title: "回答を受け取れませんでした", message: "boom" });
    expect(parseChatError(undefined)).toBeNull();
  });

  it("recognises network-level failures when the sidecar is unreachable", () => {
    for (const m of ["Failed to fetch", "NetworkError when attempting", "connect ECONNREFUSED 127.0.0.1:1", "Load failed"]) {
      expect(parseChatError(new Error(m))).toEqual({ kind: "network", provider: null, title: "エージェントに接続できません", message: m });
    }
  });
});
