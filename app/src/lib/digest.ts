import type { AppStatus, Digest, DigestView } from "../types";

const BULLET = /^\s*(?:[-*]\s+|[・•]\s*|\d+[.)]\s+)(.*)$/;

const RULE = /^(?:[-*_]\s*){3,}$/;
const MARKER_ONLY = /^[-*・•]$/;
const HEADING = /^#{1,6}\s+(.*)$/;
const BOLD = /(\*\*|__)(.+?)\1/g;

const stripBold = (text: string) => text.replace(BOLD, "$2").trim();

export function parseDigestBody(body: string): { bullets: string[]; paragraphs: string[] } {
  const bullets: string[] = [];
  const paragraphs: string[] = [];
  for (const raw of body.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || RULE.test(line) || MARKER_ONLY.test(line)) continue;
    const m = BULLET.exec(line);
    if (m) {
      const text = stripBold(m[1]);
      if (text) bullets.push(text);
      continue;
    }
    const h = HEADING.exec(line);
    const text = stripBold(h ? h[1] : line);
    if (text) paragraphs.push(text);
  }
  return { bullets, paragraphs };
}

export type DigestDisplay =
  | { kind: "done"; digest: Digest }
  | { kind: "generating" }
  | { kind: "pending"; error: string | null; attempts: number }
  | { kind: "failed"; error: string | null; attempts: number }
  | { kind: "none" };

export function digestDisplay(view: DigestView, status: AppStatus): DigestDisplay {
  const d = view.digest;
  if (d && d.status === "done") return { kind: "done", digest: d };
  if (status.digesting && (!d || d.status === "pending")) return { kind: "generating" };
  if (d && d.status === "pending") return { kind: "pending", error: d.last_error, attempts: d.attempts };
  if (d && d.status === "failed") return { kind: "failed", error: d.last_error, attempts: d.attempts };
  return { kind: "none" };
}
