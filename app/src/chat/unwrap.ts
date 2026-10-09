export type Obj = Record<string, any>;

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
