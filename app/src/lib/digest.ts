import type { AppStatus, Digest, DigestView } from "../types";

const BULLET = /^\s*(?:[-*]\s+|[・•]\s*|\d+[.)]\s+)(.*)$/;

const RULE = /^(?:[-*_]\s*){3,}$/;
const MARKER_ONLY = /^[-*・•]$/;
const HEADING = /^#{1,6}\s+(.*)$/;
// 見出しの行。見出し記号 (#) や、ラベルを囲む太字 (**見出し**: / **見出し:** / **見出し: x**) も受け付ける。
const HEADLINE = /^(?:#{1,6}\s+)?(\*\*|__)?見出し\s*(?:\1)?\s*[:：]\s*(?:\1)?\s*(.*)$/;
const BOLD = /(\*\*|__)(.+?)\1/g;

const stripBold = (text: string) => text.replace(BOLD, "$2").trim();

export function parseDigestBody(body: string): { headline: string | null; bullets: string[]; paragraphs: string[] } {
  const bullets: string[] = [];
  const paragraphs: string[] = [];
  let headline: string | null = null;
  let first = true;
  for (const raw of body.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || RULE.test(line) || MARKER_ONLY.test(line)) continue;
    // 見出しは最初の空でない行だけ。長さはモデルへの指示で、ここでは切らない。
    if (first) {
      first = false;
      const hm = HEADLINE.exec(line);
      if (hm) {
        let text = stripBold(hm[2]);
        // 行全体を太字で囲んだときに残る閉じ記号を外す。
        if (hm[1] && text.endsWith(hm[1])) text = text.slice(0, -hm[1].length).trim();
        headline = text || null;
        continue;
      }
    }
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
  return { headline, bullets, paragraphs };
}

export type DigestDisplay =
  /** `regenerating`: 本文を残したまま、この scope の再生成を依頼済み。 */
  | { kind: "done"; digest: Digest; regenerating: boolean }
  | { kind: "generating" }
  | { kind: "pending"; error: string | null; attempts: number }
  | { kind: "failed"; error: string | null; attempts: number }
  /** `itemCount`: この期間の記事数。0 なら要約する対象が無い。 */
  | { kind: "none"; itemCount: number };

/**
 * セクションの表示を決める。
 * `requested` はこの画面からこの scope の生成を依頼して、まだ完了通知が来ていないこと。
 * 依頼していない scope は、全体の `status.digesting` だけを手がかりにする (失敗は失敗のまま見せる)。
 */
export function digestDisplay(view: DigestView, status: AppStatus, requested = false): DigestDisplay {
  const d = view.digest;
  if (d && d.status === "done") return { kind: "done", digest: d, regenerating: requested };
  if (requested) return { kind: "generating" };
  if (status.digesting && (!d || d.status === "pending")) return { kind: "generating" };
  if (d && d.status === "pending") return { kind: "pending", error: d.last_error, attempts: d.attempts };
  if (d && d.status === "failed") return { kind: "failed", error: d.last_error, attempts: d.attempts };
  return { kind: "none", itemCount: view.item_count };
}
