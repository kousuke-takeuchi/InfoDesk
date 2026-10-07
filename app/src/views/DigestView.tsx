import { CircleAlert, Hourglass, Inbox, Loader, RotateCw } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { api, EVENTS } from "../api";
import { ErrorLine } from "../components/ErrorLine";
import { Segmented, ViewSwitch } from "../components/Segmented";
import type { Selection, View } from "../components/Sidebar";
import { useTauriEvent } from "../hooks/useTauriEvent";
import { localDateString, periodHeading } from "../lib/dates";
import { digestDisplay, parseDigestBody } from "../lib/digest";
import { errorMessage, type AppStatus, type DigestScope, type DigestView as DV, type Item, type Period, type Topic } from "../types";

const PERIODS: { id: Period; label: string }[] = [
  { id: "day", label: "今日" }, { id: "week", label: "週" }, { id: "month", label: "月" },
];
const REF_LIMIT = 5;

interface Props {
  selection: Selection;
  topics: Topic[];
  status: AppStatus;
  view: View;
  onSwitchView: (v: View) => void;
}

function sameScope(a: DigestScope, b: Selection): boolean {
  if (b.kind === "all") return true;
  if (b.kind === "category") return "category" in a && a.category === b.category;
  return "topic" in a && a.topic === b.topicId;
}

function References({ ids, onOpen }: { ids: number[]; onOpen: (id: number) => void }) {
  const [items, setItems] = useState<Item[]>([]);
  useEffect(() => {
    let alive = true;
    Promise.all(ids.slice(0, REF_LIMIT).map((id) => api.getItem(id).catch(() => null)))
      .then((list) => { if (alive) setItems(list.filter((i): i is Item => i !== null)); });
    return () => { alive = false; };
  }, [ids]);
  if (ids.length === 0) return null;
  return (
    <div className="digest-refs">
      <span className="text-tertiary tiny">根拠記事</span>
      {items.map((it) => <button key={it.id} className="link-btn" onClick={() => onOpen(it.id)}>{it.title}</button>)}
      {ids.length > REF_LIMIT && <span className="text-secondary">他 {ids.length - REF_LIMIT}件</span>}
    </div>
  );
}

export function DigestView({ selection, topics, status, view, onSwitchView }: Props) {
  const [period, setPeriod] = useState<Period>("day");
  const [views, setViews] = useState<DV[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const today = new Date();
  const date = localDateString(today);
  const topicId = selection.kind === "topic" ? selection.topicId : null;

  const load = useCallback(() => {
    api.getDigests(period, date, topicId)
      .then((v) => { setViews(v); setError(null); })
      .catch((e) => setError(errorMessage(e)));
  }, [period, date, topicId]);

  useEffect(load, [load]);
  useTauriEvent(EVENTS.digests, load);
  useTauriEvent(EVENTS.status, load);

  const run = (p: Promise<unknown>) => p.catch((e) => setError(errorMessage(e)));
  const heading = periodHeading(period, today);
  const shown = (views ?? []).filter((v) => sameScope(v.scope, selection));
  const scopeNote = selection.kind === "topic" ? topics.find((t) => t.id === selection.topicId)?.name ?? null : null;

  return (
    <>
      <div className="toolbar">
        <ViewSwitch view={view} onChange={onSwitchView} />
        <div className="spacer" />
        <Segmented options={PERIODS} value={period} onChange={setPeriod} ariaLabel="期間" />
      </div>
      <div className="view">
        <ErrorLine message={error} />
        <div className="view-header">
          <h1>{heading.title}</h1>
          <span className="text-secondary">{heading.range}</span>
          {scopeNote && <span className="text-secondary">· {scopeNote}</span>}
          <div className="spacer" />
          <button className="btn btn-ghost" disabled={status.digesting}
            onClick={() => shown.forEach((v) => run(api.regenerateDigest(period, date, v.scope)))}>
            <RotateCw size={14} /> 再生成
          </button>
        </div>
        {shown.map((v) => {
          const display = digestDisplay(v, status);
          const key = "category" in v.scope ? v.scope.category : `topic-${v.scope.topic}`;
          return (
            <section key={key} className="digest-section">
              <div className="digest-head">
                <h2>{v.label}</h2>
                {display.kind === "done" && (
                  <span className="text-tertiary tiny">{display.digest.provider} · 記事{display.digest.item_ids.length}件</span>
                )}
              </div>
              {display.kind === "done" && (() => {
                const { bullets, paragraphs } = parseDigestBody(display.digest.body);
                return (
                  <>
                    <ul className="digest-bullets">{bullets.map((b, i) => <li key={i}>{b}</li>)}</ul>
                    {paragraphs.map((p, i) => <p key={i} style={{ margin: 0, lineHeight: 1.6 }}>{p}</p>)}
                    <References ids={display.digest.item_ids} onOpen={(id) => run(api.openItem(id))} />
                  </>
                );
              })()}
              {display.kind === "generating" && (
                <div className="state-box"><Loader size={15} className="spin" /><span className="grow">要約を生成中…</span></div>
              )}
              {display.kind === "pending" && (
                <div className="state-box">
                  <Hourglass size={15} />
                  <span className="grow">保留中。次回のスケジュールで再試行します。{display.error ? ` 最後のエラー: ${display.error}` : ""}</span>
                  <button className="btn" onClick={() => run(api.regenerateDigest(period, date, v.scope))}>今すぐ再試行</button>
                </div>
              )}
              {display.kind === "failed" && (
                <div className="state-box">
                  <CircleAlert size={15} style={{ color: "var(--danger)" }} />
                  <span className="grow">生成に{display.attempts}回失敗しました。LLMの設定を確認してから再試行してください。{display.error ? ` 最後のエラー: ${display.error}` : ""}</span>
                  <button className="btn" onClick={() => run(api.regenerateDigest(period, date, v.scope))}>再試行</button>
                </div>
              )}
              {display.kind === "none" && (
                <div className="state-box"><Inbox size={15} /><span className="grow">この期間に記事がありません。</span></div>
              )}
            </section>
          );
        })}
      </div>
    </>
  );
}
