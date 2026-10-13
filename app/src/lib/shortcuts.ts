import type { Selection } from "../components/Sidebar";
import { CATEGORIES, type Topic } from "../types";

export type ShortcutAction =
  | "collect" | "digest" | "feed" | "periodDay" | "periodWeek" | "periodMonth"
  | "settings" | "chat" | "next" | "prev" | "open" | "toggleRead" | "toggleStar"
  | "search" | "markAllRead" | "sidebarPrev" | "sidebarNext" | "selectAll" | "help";

export interface KeyInfo {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  isComposing?: boolean;
  keyCode?: number;
  target: EventTarget | null;
}

export interface ShortcutContext {
  inSettings: boolean;
  /** `[role="dialog"][aria-modal="true"]` が開いている。 */
  dialogOpen: boolean;
}

const PLAIN: Record<string, ShortcutAction> = {
  "1": "digest", "2": "feed", d: "periodDay", w: "periodWeek", m: "periodMonth",
  j: "next", k: "prev", Enter: "open", u: "toggleRead", s: "toggleStar", "/": "search",
  "[": "sidebarPrev", "]": "sidebarNext", "0": "selectAll",
};
const MOD: Record<string, ShortcutAction> = { r: "collect", ",": "settings", j: "chat" };
const IN_SETTINGS: ReadonlySet<ShortcutAction> = new Set<ShortcutAction>(["collect", "settings", "chat", "help"]);

function isTextTarget(t: EventTarget | null): boolean {
  const el = t as HTMLElement | null;
  if (!el || typeof el.tagName !== "string") return false;
  const tag = el.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || el.isContentEditable === true;
}

function isActivatable(t: EventTarget | null): boolean {
  const tag = (t as HTMLElement | null)?.tagName;
  return tag === "BUTTON" || tag === "A" || tag === "SUMMARY";
}

/** キーイベントをアクション名に変換する。該当なし・抑止すべき状況なら null。 */
export function matchShortcut(e: KeyInfo, ctx: ShortcutContext): ShortcutAction | null {
  if (e.isComposing || e.keyCode === 229) return null;
  if (isTextTarget(e.target) || ctx.dialogOpen) return null;
  let action: ShortcutAction | null = null;
  if (e.ctrlKey || e.metaKey) {
    if (!e.altKey) action = MOD[e.key.toLowerCase()] ?? null;
  } else if (!e.altKey) {
    if (e.key === "?") action = "help";
    else if (e.key === "A" && e.shiftKey) action = "markAllRead";
    // Caps Lock 中は Shift なしでも大文字が届くので、1文字のキーは小文字にして引く。
    else if (!e.shiftKey) action = PLAIN[e.key.length === 1 ? e.key.toLowerCase() : e.key] ?? null;
  }
  if (action === "open" && isActivatable(e.target)) return null;
  if (action && ctx.inSettings && !IN_SETTINGS.has(action)) return null;
  return action;
}

function sameSelection(a: Selection, b: Selection): boolean {
  if (a.kind !== b.kind) return false;
  if (a.kind === "category" && b.kind === "category") return a.category === b.category;
  if (a.kind === "topic" && b.kind === "topic") return a.topicId === b.topicId;
  return true;
}

/** サイドバーの並び (すべて、カテゴリー、トピック)。 */
export function sidebarOrder(topics: Topic[]): Selection[] {
  return [
    { kind: "all" },
    ...CATEGORIES.map((c): Selection => ({ kind: "category", category: c.id })),
    ...topics.map((t): Selection => ({ kind: "topic", topicId: t.id })),
  ];
}

/** サイドバーの前後の項目。端では反対側へ回る。現在の項目が見つからなければ先頭から数える。 */
export function stepSelection(current: Selection, topics: Topic[], dir: 1 | -1): Selection {
  const order = sidebarOrder(topics);
  const i = order.findIndex((s) => sameSelection(s, current));
  const from = i < 0 ? 0 : i;
  return order[(from + dir + order.length) % order.length];
}
