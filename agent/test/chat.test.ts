import type { UIMessage } from "ai";
import { describe, expect, it } from "vitest";
import { OLDER_TOOL_OUTPUT_CHARS, truncateOlderToolOutputs } from "../src/chat";

function toolPart(output: unknown) {
  return { type: "tool-fetch_page", toolCallId: "c", state: "output-available", input: { url: "https://a" }, output };
}

describe("truncateOlderToolOutputs", () => {
  const big = { text: "x".repeat(5000) };
  const small = { text: "short" };
  const messages = [
    { id: "u1", role: "user", parts: [{ type: "text", text: "q1" }] },
    { id: "a1", role: "assistant", parts: [toolPart(big), toolPart(small), { type: "text", text: "a1" }] },
    { id: "u2", role: "user", parts: [{ type: "text", text: "q2" }] },
    { id: "a2", role: "assistant", parts: [toolPart(big)] },
    { id: "u3", role: "user", parts: [{ type: "text", text: "q3" }] },
  ] as unknown as UIMessage[];

  it("shortens oversized tool outputs of older assistant messages", () => {
    const out = truncateOlderToolOutputs(messages);
    const parts = out[1].parts as Array<{ output?: unknown; type: string }>;
    expect(OLDER_TOOL_OUTPUT_CHARS).toBe(2000);
    expect(parts[0].output).toEqual({ truncated: true, preview: JSON.stringify(big).slice(0, 2000) });
    expect(parts[1].output).toEqual(small);
    expect(parts[2]).toEqual({ type: "text", text: "a1" });
  });

  it("keeps the latest assistant message intact and does not mutate the input", () => {
    const out = truncateOlderToolOutputs(messages);
    expect(out[3]).toEqual(messages[3]);
    expect((out[3].parts[0] as { output?: unknown }).output).toEqual(big);
    expect((messages[1].parts[0] as { output?: unknown }).output).toEqual(big);
    expect(out[4]).toEqual(messages[4]);
  });
});
