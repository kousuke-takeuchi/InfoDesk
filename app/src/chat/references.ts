import type { UIMessage } from "ai";

type Obj = Record<string, any>;

function unwrap(output: unknown): Obj | null {
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

export function referencedItems(message: UIMessage, max = 8): { id: number; title: string }[] {
  const seen = new Map<number, string>();
  for (const part of message.parts as Obj[]) {
    const isTool = typeof part.type === "string" && (part.type.startsWith("tool-") || part.type === "dynamic-tool");
    if (!isTool || part.state !== "output-available") continue;
    const out = unwrap(part.output);
    if (!out) continue;
    const list: Obj[] = Array.isArray(out.items) ? out.items : out.item ? [out.item] : [];
    for (const it of list) {
      if (typeof it?.id === "number" && typeof it?.title === "string" && !seen.has(it.id)) seen.set(it.id, it.title);
    }
  }
  return [...seen].slice(0, max).map(([id, title]) => ({ id, title }));
}
