import type { Selection } from "../components/Sidebar";
import type { ItemFilter } from "../types";
import { addDays, startOfLocalDay } from "./dates";

export type FeedRange = "today" | "3d" | "week" | "month" | "all";

export const FEED_RANGES: { id: FeedRange; label: string; days: number | null }[] = [
  { id: "today", label: "今日", days: 0 },
  { id: "3d", label: "3日", days: 2 },
  { id: "week", label: "1週間", days: 6 },
  { id: "month", label: "1ヶ月", days: 29 },
  { id: "all", label: "すべて", days: null },
];

export interface FeedState {
  selection: Selection;
  range: FeedRange;
  sourceId: number | null;
  unreadOnly: boolean;
  starredOnly: boolean;
  text: string;
}

/** 画面の状態からバックエンドの ItemFilter を作る。全フィールドを埋める。 */
export function buildItemFilter(state: FeedState, now: Date, limit = 100): ItemFilter {
  const range = FEED_RANGES.find((r) => r.id === state.range);
  const since = range && range.days !== null ? addDays(startOfLocalDay(now), -range.days).toISOString() : null;
  const text = state.text.trim();
  return {
    category: state.selection.kind === "category" ? state.selection.category : null,
    topic_id: state.selection.kind === "topic" ? state.selection.topicId : null,
    source_id: state.sourceId,
    since,
    until: null,
    text: text.length > 0 ? text : null,
    unread_only: state.unreadOnly,
    starred_only: state.starredOnly,
    order: "recent",
    limit,
    offset: 0,
  };
}
