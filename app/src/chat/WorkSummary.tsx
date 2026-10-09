import type { UIMessage } from "ai";
import { Check, ChevronRight, Circle, LoaderCircle } from "lucide-react";
import { createContext, useEffect, useState } from "react";
import { announceLine, answerOutcome, answerTimes, displayPlan, latestPlan, statusLine, workStats } from "./progress";

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
  const { steps, tools, failed } = workStats(message);
  const { startedAt, finishedAt } = answerTimes(message);
  const outcome = answerOutcome(message, running);
  const end = running ? now : finishedAt;
  const elapsedMs = startedAt !== null && end !== null ? Math.max(0, end - startedAt) : null;
  // 作業のない古いメッセージは何も出さない。時刻のある回答は、作業がなくても終わったことを読み上げる。
  if (!running && steps === 0 && !rawPlan && outcome === "unknown") return null;
  // 書き終えずに止まった回 (ステップ数の上限、エラー、中断) は「途中で停止」と示す。古いメッセージには付けない。
  const stopped = outcome === "stopped";
  const line = statusLine({ running, elapsedMs, steps, tools, failed, stopped });
  // 見える行は毎秒変わるので読み上げない。読み上げは秒数を含まない別の行で行う。
  const announcement = announceLine({ running, steps, tools, failed, stopped });
  // 書き終えた回答は全項目を完了にする。それ以外は最後の状態のまま、回転なしで出す。
  const plan = rawPlan ? displayPlan(rawPlan, outcome === "answered") : null;
  const visible = running || steps > 0 || plan !== null;
  return (
    <div className={visible ? "work-summary" : "work-summary quiet"}>
      <div className="visually-hidden" role="status">{announcement}</div>
      {running ? (
        <div className="work-status"><LoaderCircle size={13} className="spin" aria-hidden />{line}</div>
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
