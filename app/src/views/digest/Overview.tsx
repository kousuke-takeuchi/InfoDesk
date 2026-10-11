import { Minus, TrendingDown, TrendingUp } from "lucide-react";
import { BarChart } from "../../components/charts/BarChart";
import { Sparkline } from "../../components/charts/Sparkline";
import { ErrorLine } from "../../components/ErrorLine";
import { categoryLabel, type DigestStats } from "../../types";

export function trend(count: number, prev: number): "up" | "down" | "flat" {
  return count > prev ? "up" : count < prev ? "down" : "flat";
}

function delta(count: number, prev: number): string {
  const d = count - prev;
  return d > 0 ? `+${d}` : d < 0 ? `-${-d}` : "±0";
}

const ICONS = { up: TrendingUp, down: TrendingDown, flat: Minus };

export function Overview({ stats, error }: { stats: DigestStats | null; error: string | null }) {
  if (error) return <ErrorLine message={error} />;
  if (!stats) return null;
  if (stats.total === 0) {
    return <div className="overview"><div className="state-box"><span className="grow">この期間に記事がありません</span></div></div>;
  }
  const rows = stats.categories.map((c) => {
    const label = categoryLabel(c.category);
    return {
      label, value: c.count,
      extra: stats.period !== "day" && c.daily.length > 0
        ? <Sparkline ariaLabel={`${label}の日ごとの件数`} values={c.daily} />
        : undefined,
    };
  });
  const summary = `カテゴリー別の記事数: ${rows.map((r) => `${r.label} ${r.value}件`).join("、")}`;
  return (
    <div className="overview">
      <div>
        <h3>期間の概観 <span className="text-tertiary">{stats.total}件</span></h3>
        <BarChart ariaLabel={summary} rows={rows} />
      </div>
      {stats.topics.length > 0 && (
        <div>
          <h3>話題の強さ</h3>
          <ul className="topic-trends" aria-label="話題の強さ">
            {stats.topics.map((t) => {
              const tr = trend(t.count, t.prev_count);
              const Icon = ICONS[tr];
              return (
                <li key={t.topic_id} data-trend={tr}>
                  <span>{t.name}</span>
                  <span>{t.count}件</span>
                  <span className="delta"><Icon size={12} aria-hidden="true" />{delta(t.count, t.prev_count)}</span>
                </li>
              );
            })}
          </ul>
        </div>
      )}
    </div>
  );
}
