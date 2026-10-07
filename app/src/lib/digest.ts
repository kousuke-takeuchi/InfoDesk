import type { AppStatus, Digest, DigestView } from "../types";

const BULLET = /^\s*(?:[-*]\s+|[・•]\s*|\d+[.)]\s+)(.*)$/;

export function parseDigestBody(body: string): { bullets: string[]; paragraphs: string[] } {
  const bullets: string[] = [];
  const paragraphs: string[] = [];
  for (const raw of body.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line) continue;
    const m = BULLET.exec(line);
    if (m) bullets.push(m[1].trim());
    else paragraphs.push(line);
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
