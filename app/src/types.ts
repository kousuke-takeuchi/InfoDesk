export type Category =
  | "github" | "arxiv" | "huggingface" | "hackernews" | "tech_jp" | "tech_intl" | "medcare";

export const CATEGORIES: { id: Category; label: string }[] = [
  { id: "github", label: "GitHub" },
  { id: "arxiv", label: "arXiv" },
  { id: "huggingface", label: "Hugging Face" },
  { id: "hackernews", label: "Hacker News" },
  { id: "tech_jp", label: "国内tech" },
  { id: "tech_intl", label: "海外tech" },
  { id: "medcare", label: "医療介護" },
];

export function categoryLabel(id: Category): string {
  return CATEGORIES.find((c) => c.id === id)?.label ?? id;
}

export type Period = "day" | "week" | "month";
export type ItemOrder = "recent" | "score";
export type DigestStatus = "pending" | "done" | "failed";
export type SourceKind = "github_trending" | "arxiv" | "hf_models" | "hf_papers" | "hackernews" | "rss";

export type DigestScope = { category: Category } | { topic: number };

export interface Item {
  id: number;
  source_id: number | null;
  source_name: string;
  category: Category;
  url: string;
  title: string;
  excerpt: string;
  author: string | null;
  published_at: string | null;
  fetched_at: string;
  score: number | null;
  ai_summary: string | null;
  read: boolean;
  starred: boolean;
  image_url: string | null;
}

export interface ItemFilter {
  category: Category | null;
  topic_id: number | null;
  source_id: number | null;
  since: string | null;
  until: string | null;
  text: string | null;
  unread_only: boolean;
  starred_only: boolean;
  order: ItemOrder;
  limit: number;
  offset: number;
}

export interface Source {
  id: number;
  kind: SourceKind;
  name: string;
  category: Category;
  config: string;
  enabled: boolean;
  last_fetched_at: string | null;
  last_error: string | null;
}

export interface Topic {
  id: number;
  name: string;
  keywords: string[];
}

export interface Digest {
  id: number;
  period: Period;
  period_start: string;
  scope: DigestScope;
  body: string;
  item_ids: number[];
  provider: string;
  status: DigestStatus;
  attempts: number;
  created_at: string;
  last_error: string | null;
}

export interface DigestView {
  scope: DigestScope;
  label: string;
  digest: Digest | null;
  /** 取得日時がこの期間に入る記事の数。 */
  item_count: number;
}

export interface CategoryStat {
  category: Category;
  count: number;
  /** 日の期間では空。期間の初日から順。 */
  daily: number[];
}

export interface TopicStat {
  topic_id: number;
  name: string;
  count: number;
  prev_count: number;
}

export interface DigestStats {
  period: Period;
  period_start: string;
  total: number;
  categories: CategoryStat[];
  topics: TopicStat[];
}

export interface AppStatus {
  collecting: boolean;
  summarizing: boolean;
  digesting: boolean;
  last_collect_at: string | null;
  next_collect_at: string | null;
  last_error: string | null;
}

export interface SidebarCounts {
  total_unread: number;
  categories: Record<string, number>;
  topics: Record<string, number>;
}

export interface ProbeResult {
  ok: boolean;
  message: string;
  millis: number;
}

export type Settings = Record<string, string>;

/** バックエンドの AppError。invoke の reject 値。 */
export interface AppError {
  message: string;
}

export function errorMessage(e: unknown): string {
  if (e && typeof e === "object" && "message" in e && typeof (e as AppError).message === "string") {
    return (e as AppError).message;
  }
  return String(e);
}

export interface ChatThread {
  id: number;
  title: string;
  created_at: string;
  updated_at: string;
}

export type ChatEndpoint =
  | { status: "starting" }
  | { status: "ready"; url: string; token: string }
  | { status: "error"; kind: "node_missing" | "node_too_old" | "agent_missing" | "start_failed"; message: string };
