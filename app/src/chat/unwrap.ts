export type Obj = Record<string, any>;

export const MCP_PREFIX = "mcp__infodesk__";

export const obj = (v: unknown): Obj => (v && typeof v === "object" ? (v as Obj) : {});

// Tool names arrive bare from the Anthropic API and prefixed from the Claude Agent SDK.
export function stripMcpPrefix(name: string): string {
  return name.startsWith(MCP_PREFIX) ? name.slice(MCP_PREFIX.length) : name;
}

// Opens a tool result that may be an object, a JSON string, or an MCP text block.
export function unwrap(output: unknown): Obj | null {
  let v: unknown = output;
  if (v && typeof v === "object" && Array.isArray((v as Obj).content)) {
    const text = (v as Obj).content.find((c: Obj) => c?.type === "text")?.text;
    v = typeof text === "string" ? text : null;
  }
  if (typeof v === "string") {
    try { v = JSON.parse(v); } catch { return null; }
  }
  return v && typeof v === "object" ? (v as Obj) : null;
}
