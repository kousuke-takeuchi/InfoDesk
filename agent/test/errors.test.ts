import { describe, expect, it } from "vitest";
import { CliError } from "../src/cli";
import { describeError } from "../src/errors";

const S = { "ollama.base_url": "http://localhost:11434", "ollama.model": "gemma4:26b" };
const parse = (s: string) => JSON.parse(s);

describe("describeError", () => {
  it("recognizes an unreachable Ollama", () => {
    const e = Object.assign(new TypeError("fetch failed"), { cause: { code: "ECONNREFUSED" } });
    expect(parse(describeError(e, "ollama", S))).toEqual({
      kind: "llm_unreachable", provider: "ollama",
      message: "http://localhost:11434 が応答しません。Ollama が起動しているか確認してください。",
    });
  });

  it("recognizes a missing Ollama model", () => {
    const e = new Error("model 'gemma4:26b' not found");
    expect(parse(describeError(e, "ollama", S))).toMatchObject({ kind: "model_missing", message: expect.stringContaining("gemma4:26b") });
  });

  it("recognizes a Claude login problem", () => {
    const e = new Error("Authentication failed: please run claude login");
    expect(parse(describeError(e, "claude", S))).toMatchObject({ kind: "auth", provider: "claude" });
  });

  it("recognizes other Claude auth wordings but not a bare 401", () => {
    expect(parse(describeError(new Error("401 Unauthorized"), "claude", S))).toMatchObject({ kind: "auth" });
    expect(parse(describeError(new Error("Please log in again"), "claude", S))).toMatchObject({ kind: "auth" });
    expect(parse(describeError(new Error("request 4012 failed"), "claude", S))).toMatchObject({ kind: "other" });
    expect(parse(describeError(new Error("HTTP 401"), "claude", S))).toMatchObject({ kind: "other" });
    expect(parse(describeError(new Error("blogin catalog"), "claude", S))).toMatchObject({ kind: "other" });
  });

  it("names a CLI failure as a tool error", () => {
    for (const provider of ["ollama", "claude"] as const) {
      expect(parse(describeError(new CliError("infodesk CLI timed out"), provider, S))).toEqual({
        kind: "tool", provider, message: "infodesk CLI を実行できません: infodesk CLI timed out",
      });
    }
  });

  it("passes other errors through", () => {
    expect(parse(describeError(new Error("boom"), "claude", S))).toEqual({ kind: "other", provider: "claude", message: "boom" });
    expect(parse(describeError("text", "ollama", S))).toEqual({ kind: "other", provider: "ollama", message: "text" });
  });
});
