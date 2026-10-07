import {
  Box, FileText, Flame, GitBranch, Globe, Hash, HeartPulse, Layers, Newspaper, RefreshCw, Settings,
} from "lucide-react";
import type { ComponentType } from "react";
import { CATEGORIES, type AppStatus, type Category, type SidebarCounts, type Topic } from "../types";
import { formatClock } from "../lib/dates";

export type Selection =
  | { kind: "all" }
  | { kind: "category"; category: Category }
  | { kind: "topic"; topicId: number };

export type View = "digest" | "feed" | "settings";

const ICONS: Record<Category, ComponentType<{ size?: number }>> = {
  github: GitBranch, arxiv: FileText, huggingface: Box, hackernews: Flame,
  tech_jp: Newspaper, tech_intl: Globe, medcare: HeartPulse,
};

interface Props {
  selection: Selection;
  view: View;
  counts: SidebarCounts;
  topics: Topic[];
  status: AppStatus;
  onSelect: (s: Selection) => void;
  onOpenSettings: () => void;
  onCollectNow: () => void;
}

function sameSelection(a: Selection, b: Selection): boolean {
  if (a.kind !== b.kind) return false;
  if (a.kind === "category" && b.kind === "category") return a.category === b.category;
  if (a.kind === "topic" && b.kind === "topic") return a.topicId === b.topicId;
  return true;
}

function NavItem({ icon: Icon, label, count, current, onClick }: {
  icon: ComponentType<{ size?: number }>; label: string; count?: number; current: boolean; onClick: () => void;
}) {
  return (
    <button className="nav-item" aria-current={current ? "true" : undefined} onClick={onClick}>
      <Icon size={16} />
      <span className="label">{label}</span>
      {count ? <span className="count">{count}</span> : null}
    </button>
  );
}

export function Sidebar({ selection, view, counts, topics, status, onSelect, onOpenSettings, onCollectNow }: Props) {
  const inContent = view !== "settings";
  const current = (s: Selection) => inContent && sameSelection(selection, s);
  return (
    <nav className="sidebar" aria-label="ナビゲーション">
      <NavItem icon={Layers} label="すべて" count={counts.total_unread}
        current={current({ kind: "all" })} onClick={() => onSelect({ kind: "all" })} />
      <div className="sidebar-head">カテゴリー</div>
      {CATEGORIES.map((c) => (
        <NavItem key={c.id} icon={ICONS[c.id]} label={c.label} count={counts.categories[c.id] ?? 0}
          current={current({ kind: "category", category: c.id })}
          onClick={() => onSelect({ kind: "category", category: c.id })} />
      ))}
      {topics.length > 0 && <div className="sidebar-head">トピック</div>}
      {topics.map((t) => (
        <NavItem key={t.id} icon={Hash} label={t.name} count={counts.topics[String(t.id)] ?? 0}
          current={current({ kind: "topic", topicId: t.id })}
          onClick={() => onSelect({ kind: "topic", topicId: t.id })} />
      ))}
      <div className="sidebar-spacer" />
      <button className="sidebar-status" onClick={onCollectNow} disabled={status.collecting} title="今すぐ収集">
        <RefreshCw size={14} className={status.collecting ? "spin" : undefined} />
        <span>
          {status.collecting ? "収集中…" : status.last_collect_at ? `最終収集 ${formatClock(status.last_collect_at)}` : "未収集"}
        </span>
      </button>
      {status.last_error && <div className="sidebar-error">{status.last_error}</div>}
      <NavItem icon={Settings} label="設定" current={view === "settings"} onClick={onOpenSettings} />
    </nav>
  );
}
