import type { Digest } from "../types";

/** 日次要約を新聞の版として見たもの。issue は版の名前の日 (朝刊は D+1、夕刊は D)、articles は記事の日 (D)。 */
export type Edition =
  | { kind: "morning"; issue: Date; articles: Date }
  | { kind: "evening"; issue: Date; articles: Date; cutoff: Date | null };

/**
 * `start` (YYYY-MM-DD) の日次要約の版。完了した要約の created_at の最新で決める。
 * D のうちなら D の夕刊、D が終わった後なら D+1 の朝刊 (D の確定版)。完了した要約がない、
 * または時刻を読めないときは null。
 */
export function editionOf(start: string, digests: (Digest | null)[]): Edition | null {
  const [y, m, d] = start.split("-").map(Number);
  const day = new Date(y, m - 1, d);
  const next = new Date(y, m - 1, d + 1);
  let newest: Date | null = null;
  for (const g of digests) {
    if (!g || g.status !== "done") continue;
    const t = new Date(g.created_at);
    if (Number.isNaN(t.getTime())) continue;
    if (!newest || t > newest) newest = t;
  }
  if (!newest) return null;
  if (newest >= next) return { kind: "morning", issue: next, articles: day };
  return { kind: "evening", issue: day, articles: day, cutoff: newest < day ? null : newest };
}

/** 「11月2日 朝刊」/「11月1日の記事」、「11月2日 夕刊」/「11月2日 17時までの記事」。年は今と違うときだけ付ける。 */
export function editionHeading(e: Edition, now: Date): { title: string; range: string } {
  const md = (d: Date) => `${d.getFullYear() === now.getFullYear() ? "" : `${d.getFullYear()}年`}${d.getMonth() + 1}月${d.getDate()}日`;
  if (e.kind === "morning") return { title: `${md(e.issue)} 朝刊`, range: `${md(e.articles)}の記事` };
  const c = e.cutoff;
  const until = c ? ` ${c.getHours()}時${c.getMinutes() === 0 ? "" : `${c.getMinutes()}分`}までの記事` : "の記事";
  return { title: `${md(e.issue)} 夕刊`, range: `${md(e.articles)}${until}` };
}
