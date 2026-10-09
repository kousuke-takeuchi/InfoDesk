import type { ToolCallMessagePartProps } from "@assistant-ui/react";
import { FileText, Globe, Newspaper, Search, Wrench } from "lucide-react";
import { describeTool } from "./toolLabels";

const ICONS: Record<string, typeof Search> = { 記事検索: Search, 記事を読む: Newspaper, 要約を読む: FileText, 本文取得: FileText, Web検索: Globe };

export function ToolCall({ toolName, args, result, isError, status }: ToolCallMessagePartProps) {
  const { label, detail } = describeTool(toolName, args, result);
  const Icon = ICONS[label] ?? Wrench;
  const state =
    status.type === "incomplete" && status.reason === "cancelled" ? "中断"
    : isError || status.type === "incomplete" || status.type === "requires-action" ? "失敗"
    : status.type === "running" ? "実行中"
    : "完了";
  return (
    <div className={`tool-call${state === "失敗" ? " is-error" : ""}`}>
      <Icon size={13} aria-hidden />
      {/* コロンは名前に付けて、flex の gap が前に入らないようにする (「記事検索: 認知症」) */}
      <span className="tool-label">{label}{detail && <span aria-hidden>:</span>}</span>
      {detail && <span className="tool-detail">{detail}</span>}
      <span className={`tool-state${state === "実行中" ? " running" : ""}`}>{state}</span>
    </div>
  );
}
