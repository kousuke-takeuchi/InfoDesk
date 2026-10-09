export type DigestPeriod = "day" | "week" | "month";
export type FeedRange = "today" | "3d" | "week" | "month" | "all";

export type ChatScope =
  | { kind: "all" }
  | { kind: "category"; category: string; label: string }
  | { kind: "topic"; topicId: number; label: string };

export type ChatPeriod =
  | { view: "digest"; period: DigestPeriod; date: string }
  | { view: "feed"; range: FeedRange };

export type ChatAttachment =
  | { type: "item"; id: number; title: string }
  | { type: "digest"; period: DigestPeriod; date: string; scope: string; label: string };

export interface ChatContext {
  scope: ChatScope;
  period: ChatPeriod | null;
  attachments: ChatAttachment[];
}
export type ProviderId = "ollama" | "claude";
