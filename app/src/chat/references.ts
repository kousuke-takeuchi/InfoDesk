import type { UIMessage } from "ai";

import { unwrap, type Obj } from "./unwrap";

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
