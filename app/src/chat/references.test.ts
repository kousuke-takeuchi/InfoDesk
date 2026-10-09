import type { UIMessage } from "ai";
import { describe, expect, it } from "vitest";
import { referencedItems } from "./references";

const msg = (parts: unknown[]): UIMessage => ({ id: "a", role: "assistant", parts } as UIMessage);

describe("referencedItems", () => {
  it("collects items from search and article tool outputs, deduplicated", () => {
    const m = msg([
      { type: "tool-search_articles", toolCallId: "1", state: "output-available", input: {},
        output: { items: [{ id: 1, title: "A" }, { id: 2, title: "B" }], total: 2 } },
      { type: "tool-get_article", toolCallId: "2", state: "output-available", input: { id: 2 },
        output: { item: { id: 2, title: "B" } } },
      { type: "text", text: "回答" },
    ]);
    expect(referencedItems(m)).toEqual([{ id: 1, title: "A" }, { id: 2, title: "B" }]);
  });

  it("reads Claude MCP results that arrive as text content", () => {
    const m = msg([
      { type: "dynamic-tool", toolName: "mcp__infodesk__search_articles", toolCallId: "1", state: "output-available", input: {},
        output: { content: [{ type: "text", text: JSON.stringify({ items: [{ id: 7, title: "C" }], total: 1 }) }] } },
      { type: "dynamic-tool", toolName: "mcp__infodesk__get_article", toolCallId: "2", state: "output-available", input: {},
        output: JSON.stringify({ item: { id: 8, title: "D" } }) },
    ]);
    expect(referencedItems(m)).toEqual([{ id: 7, title: "C" }, { id: 8, title: "D" }]);
  });

  it("ignores failed or unfinished tools and caps the list", () => {
    const many = Array.from({ length: 12 }, (_, i) => ({ id: i + 1, title: `T${i + 1}` }));
    const m = msg([
      { type: "tool-search_articles", toolCallId: "1", state: "output-error", input: {}, errorText: "x" },
      { type: "tool-search_articles", toolCallId: "2", state: "input-available", input: {} },
      { type: "tool-search_articles", toolCallId: "3", state: "output-available", input: {}, output: { items: many, total: 12 } },
    ]);
    expect(referencedItems(m)).toHaveLength(8);
    expect(referencedItems(m, 3).map((x) => x.id)).toEqual([1, 2, 3]);
    expect(referencedItems(msg([{ type: "text", text: "x" }]))).toEqual([]);
  });
});
