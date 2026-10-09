import type { ToolCallMessagePartProps } from "@assistant-ui/react";
import { FileText, Globe, Newspaper, Search, Wrench } from "lucide-react";
import { Fragment, useContext, useState } from "react";
import { isPlanTool } from "./progress";
import { toolDetails } from "./toolDetails";
import { describeTool } from "./toolLabels";
import { WorkCollapsedContext } from "./WorkSummary";

const ICONS: Record<string, typeof Search> = { 記事検索: Search, 記事を読む: Newspaper, 要約を読む: FileText, 本文取得: FileText, Web検索: Globe };

function errorMessage(result: unknown): string {
  const e = (result as { error?: unknown } | null | undefined)?.error;
  if (typeof e === "string" && e) return e;
  return typeof result === "string" && result ? result : "失敗しました";
}

export function ToolCall({ toolName, args, result, isError, status }: ToolCallMessagePartProps) {
  const collapsed = useContext(WorkCollapsedContext);
  const [open, setOpen] = useState(false);
  if (isPlanTool(toolName) || collapsed) return null;
  const { label, detail } = describeTool(toolName, args, result);
  const Icon = ICONS[label] ?? Wrench;
  const state =
    status.type === "incomplete" && status.reason === "cancelled" ? "中断"
    : isError || status.type === "incomplete" || status.type === "requires-action" ? "失敗"
    : status.type === "running" ? "実行中"
    : "完了";
  // 失敗したツールのエラー文は、assistant-ui が result の { error } に入れて渡す。
  const errorText = isError ? errorMessage(result) : undefined;
  const rows = toolDetails(toolName, args, result, errorText);
  const className = `tool-call${state === "失敗" ? " is-error" : ""}`;
  const content = (
    <>
      <Icon size={13} aria-hidden />
      {/* コロンは名前に付けて、flex の gap が前に入らないようにする (「記事検索: 認知症」) */}
      <span className="tool-label">{label}{detail && <span aria-hidden>:</span>}</span>
      {detail && <span className="tool-detail">{detail}</span>}
      <span className={`tool-state${state === "実行中" ? " running" : ""}`}>{state}</span>
    </>
  );
  // 開いても何も出ない呼び出しは、押せない行にする。
  if (rows.length === 0) return <div className={className}>{content}</div>;
  return (
    <>
    <button className={className} aria-expanded={open} onClick={() => setOpen((v) => !v)}>{content}</button>
    {open && (
      <dl className="tool-details">
        {rows.map((r, i) => (<Fragment key={i}><dt>{r.label}</dt><dd>{r.value}</dd></Fragment>))}
      </dl>
    )}
    </>
  );
}
