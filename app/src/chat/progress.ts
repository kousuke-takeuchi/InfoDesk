import type { UIMessage } from "ai";

export type PlanItem = { title: string; status: "pending" | "in_progress" | "done" };

import { stripMcpPrefix, type Obj } from "./unwrap";

const STATUSES = ["pending", "in_progress", "done"];
const MAX_ITEMS = 8;
const MAX_TITLE = 80;
// A streaming input is partial; a rejected or denied update never became the plan.
const SKIPPED_STATES = ["input-streaming", "output-error", "output-denied"];

export function isPlanTool(name: string): boolean {
  return stripMcpPrefix(name) === "update_plan";
}

function toolName(part: Obj): string | null {
  if (part.type === "dynamic-tool") return typeof part.toolName === "string" ? part.toolName : null;
  if (typeof part.type === "string" && part.type.startsWith("tool-")) return part.type.slice(5);
  return null;
}

function validPlan(raw: unknown): PlanItem[] | null {
  if (!Array.isArray(raw) || raw.length < 1 || raw.length > MAX_ITEMS) return null;
  const items: PlanItem[] = [];
  for (const it of raw) {
    const title = it?.title;
    if (typeof title !== "string" || !title.trim() || title.length > MAX_TITLE) return null;
    if (!STATUSES.includes(it?.status)) return null;
    items.push({ title, status: it.status });
  }
  return items;
}

export function latestPlan(message: UIMessage): PlanItem[] | null {
  let latest: PlanItem[] | null = null;
  for (const part of message.parts as Obj[]) {
    const name = toolName(part);
    if (!name || !isPlanTool(name) || SKIPPED_STATES.includes(part.state)) continue;
    latest = validPlan(part.input?.items) ?? latest;
  }
  return latest;
}

// The model may skip a final all-done update, so a finished answer shows every item done.
export function displayPlan(plan: PlanItem[], finished: boolean): PlanItem[] {
  return finished ? plan.map((it) => ({ ...it, status: "done" as const })) : plan;
}

export function workStats(message: UIMessage): { steps: number; tools: number } {
  let steps = 0;
  let tools = 0;
  for (const part of message.parts as Obj[]) {
    if (part.type === "reasoning" && typeof part.text === "string" && part.text.trim()) steps++;
    const name = toolName(part);
    if (name && !isPlanTool(name)) {
      steps++;
      tools++;
    }
  }
  return { steps, tools };
}

export function answerTimes(message: UIMessage): { startedAt: number | null; finishedAt: number | null } {
  const meta = (message.metadata ?? {}) as Obj;
  const time = (v: unknown) => {
    const t = typeof v === "string" ? Date.parse(v) : NaN;
    return Number.isNaN(t) ? null : t;
  };
  return { startedAt: time(meta.startedAt), finishedAt: time(meta.finishedAt) };
}

export function formatSeconds(ms: number): string {
  const total = Math.max(0, Math.round(ms / 1000));
  if (total < 60) return `${total}秒`;
  return `${Math.floor(total / 60)}分${String(total % 60).padStart(2, "0")}秒`;
}

export function statusLine(args: { running: boolean; elapsedMs: number | null; steps: number; tools: number }): string {
  const { running, elapsedMs, steps, tools } = args;
  const time = elapsedMs === null ? null : formatSeconds(elapsedMs);
  const parts = running
    ? ["調査中", time, tools > 0 ? `ツール${tools}回` : null]
    : [`${steps}ステップ`, time];
  return parts.filter((p): p is string => p !== null).join(" · ");
}
