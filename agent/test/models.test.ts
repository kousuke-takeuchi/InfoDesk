import { describe, expect, it } from "vitest";
import { MAX_STEPS } from "../src/chat";
import { CLAUDE_MAX_TURNS, defaultModels, OLLAMA_NUM_CTX } from "../src/models";

describe("defaultModels.ollama", () => {
  it("runs the chat model with the same context size as core", () => {
    expect(OLLAMA_NUM_CTX).toBe(16384);
    const model = defaultModels.ollama({ "ollama.model": "gemma4:26b" }) as unknown as {
      modelId: string;
      settings: { options?: { num_ctx?: number } };
    };
    expect(model.modelId).toBe("gemma4:26b");
    expect(model.settings.options?.num_ctx).toBe(OLLAMA_NUM_CTX);
  });
});

describe("step budget", () => {
  it("leaves room for plan updates on both providers", () => {
    expect(MAX_STEPS).toBe(20);
    expect(CLAUDE_MAX_TURNS).toBe(20);
    const model = defaultModels.claude({}, { instructions: "x", mcpServers: {}, allowedTools: [] }) as unknown as {
      settings: { maxTurns?: number };
    };
    expect(model.settings.maxTurns).toBe(20);
  });
});
