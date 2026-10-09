import type { UIMessage } from "ai";
import { Check, ChevronRight, Circle, LoaderCircle } from "lucide-react";
import { createContext, useEffect, useState } from "react";
import { answerTimes, displayPlan, latestPlan, statusLine, workStats } from "./progress";

/** 完了した回答で作業 (思考とツール) を隠すか。回答中は false。 */
export const WorkCollapsedContext = createContext(false);

function useNow(active: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [active]);
  return now;
}

const ICON = { done: Check, in_progress: LoaderCircle, pending: Circle } as const;

export function WorkSummary({ message, running, collapsed, onToggle }: {
  message: UIMessage; running: boolean; collapsed: boolean; onToggle(): void;
}) {
  const now = useNow(running);
  const rawPlan = latestPlan(message);
  const { steps, tools } = workStats(message);
  const { startedAt, finishedAt } = answerTimes(message);
  const end = running ? now : finishedAt;
  const elapsedMs = startedAt !== null && end !== null ? Math.max(0, end - startedAt) : null;
  if (!running && steps === 0 && !rawPlan) return null;
  const line = statusLine({ running, elapsedMs, steps, tools });
  // 正常に終わった回答は全項目を完了にする。途中で止まった回答は最後の状態のまま、回転なしで出す。
  const finished = !running && finishedAt !== null;
  const plan = rawPlan ? displayPlan(rawPlan, finished) : null;
  return (
    <div className="work-summary">
      {running ? (
        <div className="work-status" role="status"><LoaderCircle size={13} className="spin" aria-hidden />{line}</div>
      ) : steps > 0 ? (
        <button className="work-status" aria-expanded={!collapsed} onClick={onToggle}>
          <ChevronRight size={13} className={collapsed ? "" : "open"} aria-hidden />{line}
        </button>
      ) : null}
      {plan && (
        <ul className="work-plan" aria-label="作業計画">
          {plan.map((p, i) => {
            const Icon = ICON[p.status];
            return (
              <li key={i} data-status={p.status}>
                <Icon size={13} aria-hidden className={p.status === "in_progress" && running ? "spin" : undefined} />{p.title}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
