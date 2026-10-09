import type { UIMessage } from "ai";

export type PlanItem = { title: string; status: "pending" | "in_progress" | "done" };

type Obj = Record<string, any>;
const MCP_PREFIX = "mcp__infodesk__";
const STATUSES = ["pending", "in_progress", "done"];

export function isPlanTool(name: string): boolean {
  return (name.startsWith(MCP_PREFIX) ? name.slice(MCP_PREFIX.length) : name) === "update_plan";
}

function toolName(part: Obj): string | null {
  if (part.type === "dynamic-tool") return typeof part.toolName === "string" ? part.toolName : null;
  if (typeof part.type === "string" && part.type.startsWith("tool-")) return part.type.slice(5);
  return null;
}

export function latestPlan(message: UIMessage): PlanItem[] | null {
  let last: unknown;
  let found = false;
  for (const part of message.parts as Obj[]) {
    const name = toolName(part);
    if (name && isPlanTool(name)) {
      last = part.input?.items;
      found = true;
    }
  }
  if (!found || !Array.isArray(last)) return null;
  const items: PlanItem[] = [];
  for (const it of last) {
    if (typeof it?.title !== "string" || !STATUSES.includes(it?.status)) return null;
    items.push({ title: it.title, status: it.status });
  }
  return items;
}

// The model may skip a final all-done update, so a finished answer shows every item done.
export function displayPlan(plan: PlanItem[], finished: boolean): PlanItem[] {
  return finished ? plan.map((it) => ({ ...it, status: "done" as const })) : plan;
}

export function workStats(message: UIMessage): { steps: number; tools: number } {
  let steps = 0;
  let tools = 0;
  for (const part of message.parts as Obj[]) {
    if (part.type === "reasoning") steps++;
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
