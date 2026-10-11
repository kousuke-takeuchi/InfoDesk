import type { ReactNode } from "react";

interface Row { label: string; value: number; extra?: ReactNode }

export function BarChart({ rows, max, ariaLabel }: { rows: Row[]; max?: number; ariaLabel: string }) {
  const top = max ?? Math.max(0, ...rows.map((r) => r.value));
  return (
    <div className="bar-chart" role="img" aria-label={ariaLabel}>
      {rows.map((r) => {
        const ratio = top > 0 ? r.value / top : 0;
        return (
          <div key={r.label} className="bar-row">
            <span className="bar-label">{r.label}</span>
            <span className="bar-track"><span className="bar-fill" data-bar data-ratio={ratio} style={{ width: `${ratio * 100}%` }} /></span>
            <span className="bar-value">{r.value}</span>
            {r.extra}
          </div>
        );
      })}
    </div>
  );
}
