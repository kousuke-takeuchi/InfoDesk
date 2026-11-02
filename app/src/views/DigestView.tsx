import { ChevronLeft, ChevronRight, CircleAlert, Hourglass, Inbox, Loader, MessageSquarePlus, RotateCw } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { api, EVENTS } from "../api";
import { ChatToggle } from "../components/ChatToggle";
import { ErrorLine } from "../components/ErrorLine";
import { Segmented, ViewSwitch } from "../components/Segmented";
import type { Selection, View } from "../components/Sidebar";
import type { ViewActionsRef } from "../hooks/useShortcuts";
import { useTauriEvent } from "../hooks/useTauriEvent";
import type { ChatAttachment, ChatPeriod } from "../chat/context";
import { localDateString, parseLocalDate, periodHeading, periodStart } from "../lib/dates";
import { editionHeading, editionOf } from "../lib/editions";
import { browseStarts, pickStart } from "../lib/browse";
import { digestDisplay, parseDigestBody } from "../lib/digest";
import { ImageCards, cardItems } from "./digest/ImageCards";
import { Overview } from "./digest/Overview";
import { errorMessage, type AppStatus, type DigestScope, type DigestStats, type DigestView as DV, type Item, type Period, type Topic } from "../types";

const PERIODS: { id: Period; label: string }[] = [
  { id: "day", label: "今日" }, { id: "week", label: "週" }, { id: "month", label: "月" },
];
const REF_LIMIT = 5;
const REF_FETCH = 12;
const NO_KEYS: ReadonlySet<string> = new Set();

interface Props {
  selection: Selection;
  topics: Topic[];
  status: AppStatus;
  view: View;
  onSwitchView: (v: View) => void;
  /** ショートカットのうち、この画面で処理するものを登録する。 */
  actions?: ViewActionsRef;
  /** 増えるたびに取り直す (ウィンドウ復帰・日付変更)。 */
  refreshTick?: number;
  chatOpen: boolean;
  onToggleChat: () => void;
  onAskChat: (a: ChatAttachment) => void;
  onPeriodChange: (p: ChatPeriod) => void;
}

function sameScope(a: DigestScope, b: Selection): boolean {
  if (b.kind === "all") return true;
  if (b.kind === "category") return "category" in a && a.category === b.category;
  return "topic" in a && a.topic === b.topicId;
}

function scopeKey(scope: DigestScope): string {
  return "category" in scope ? scope.category : `topic-${scope.topic}`;
}

function selectionKey(s: Selection): string {
  if (s.kind === "category") return `category-${s.category}`;
  if (s.kind === "topic") return `topic-${s.topicId}`;
  return "all";
}

function References({ ids, onOpen }: { ids: number[]; onOpen: (id: number) => void }) {
  const [items, setItems] = useState<Item[]>([]);
  // 画像が読み込めなかった記事。カードから外し、文字のリンクに戻す。
  const [broken, setBroken] = useState<ReadonlySet<number>>(new Set());
  // 再読み込みのたびに配列は作り直される。中身が同じなら取り直さないよう、id の並びをキーにする。
  const refKey = ids.slice(0, REF_FETCH).join(",");
  useEffect(() => {
    let alive = true;
    const refIds = refKey === "" ? [] : refKey.split(",").map(Number);
    Promise.all(refIds.map((id) => api.getItem(id).catch(() => null)))
      .then((list) => { if (alive) setItems(list.filter((i): i is Item => i !== null)); });
    return () => { alive = false; };
  }, [refKey]);
  if (items.length === 0) return null;
  const carded = new Set(cardItems(items).filter((i) => !broken.has(i.id)).map((i) => i.id));
  const links = items.filter((i) => !carded.has(i.id));
  const shown = links.slice(0, REF_LIMIT);
  const more = ids.length - carded.size - shown.length;
  return (
    <>
      <ImageCards items={items} broken={broken} onBroken={(id) => setBroken((prev) => new Set(prev).add(id))} onOpen={onOpen} />
      {/* リンクが全部カードになっても、残りの件数は出す。 */}
      {(shown.length > 0 || more > 0) && (
        <div className="digest-refs">
          <span className="text-tertiary tiny">根拠記事</span>
          {shown.map((it) => <button key={it.id} className="link-btn" onClick={() => onOpen(it.id)}>{it.title}</button>)}
          {more > 0 && <span className="text-secondary">他 {more}件</span>}
        </div>
      )}
    </>
  );
}

export function DigestView({ selection, topics, status, view, onSwitchView, actions, refreshTick = 0, chatOpen, onToggleChat, onAskChat, onPeriodChange }: Props) {
  const [period, setPeriod] = useState<Period>("day");
  // 取得結果と依頼中の scope は、どの条件のものかを一緒に持つ。条件が変わった描画では
  // 即座に無効になるので、前の期間のセクションが新しい見出しの下に残らない。
  const [loaded, setLoaded] = useState<{ key: string; views: DV[] } | null>(null);
  const [requests, setRequests] = useState<{ context: string; keys: ReadonlySet<string> }>({ context: "", keys: NO_KEYS });
  const [error, setError] = useState<string | null>(null);
  const [stats, setStats] = useState<{ key: string; value: DigestStats | null; error: string | null } | null>(null);
  const latestLoad = useRef(0);
  const latestStats = useRef(0);
  // 完了通知による再取得が、応答を反映する前に別の再取得に追い越されても、依頼中の印を外し損ねないよう覚えておく。
  const settlePending = useRef(false);
  // 保存済みの期間の開始 (古い順)。どの条件のものかを一緒に持つ。
  const [starts, setStarts] = useState<{ key: string; list: string[] } | null>(null);
  const [startsError, setStartsError] = useState<string | null>(null);
  // 矢印で選んだ開始。null か別の期間のものなら、最新の版に追従する。
  const [pinned, setPinned] = useState<{ period: Period; start: string } | null>(null);
  const latestStarts = useRef(0);
  const today = new Date();
  const current = localDateString(periodStart(period, today));
  const topicId = selection.kind === "topic" ? selection.topicId : null;
  const startsKey = `${period}|${topicId ?? ""}|${current}`;
  const stored = starts && starts.key === startsKey ? starts.list : null;
  const keep = pinned && pinned.period === period ? pinned.start : null;
  // 保存済みの開始が届くまでは取らない (今日を取ってから前の日に切り替えないため)。
  const date = stored === null ? null : pickStart(stored, current, keep);
  const walk = stored === null ? [] : browseStarts(stored, current);
  const index = date === null ? -1 : walk.indexOf(date);
  // 親が毎回新しい関数を渡しても、期間か日付が変わったときだけ通知する。
  const periodChange = useRef(onPeriodChange);
  periodChange.current = onPeriodChange;
  useEffect(() => { if (date !== null) periodChange.current({ view: "digest", period, date }); }, [period, date]);
  const loadKey = `${period}|${date ?? ""}|${topicId ?? ""}`;
  const context = `${loadKey}|${selectionKey(selection)}`;
  const views = loaded && loaded.key === loadKey ? loaded.views : null;
  const requested = requests.context === context ? requests.keys : NO_KEYS;

  /** `finished`: 生成ジョブの完了通知による再取得。結果を反映するときに依頼中の印を外す。 */
  const load = useCallback((finished: boolean) => {
    if (date === null) return;
    const mine = ++latestLoad.current;
    if (finished) settlePending.current = true;
    const settle = () => {
      if (!settlePending.current) return;
      settlePending.current = false;
      setRequests({ context: "", keys: NO_KEYS });
    };
    api.getDigests(period, date, topicId)
      .then((v) => {
        // 後から出した要求があるなら、この応答は古い。
        if (latestLoad.current !== mine) return;
        setLoaded({ key: loadKey, views: v });
        setError(null);
        settle();
      })
      .catch((e) => {
        if (latestLoad.current !== mine) return;
        setError(errorMessage(e));
        settle();
      });
  }, [period, date, topicId, loadKey]);

  /** 概観の統計。要約とは別に取り、最新の要求だけを反映する。 */
  const loadStats = useCallback(() => {
    if (date === null) return;
    const mine = ++latestStats.current;
    api.digestStats(period, date, topicId)
      .then((v) => { if (latestStats.current === mine) setStats({ key: loadKey, value: v, error: null }); })
      .catch((e) => { if (latestStats.current === mine) setStats({ key: loadKey, value: null, error: errorMessage(e) }); });
  }, [period, date, topicId, loadKey]);

  /** 保存済みの期間の開始。失敗したら今の期間だけで続ける。 */
  const loadStarts = useCallback(() => {
    const mine = ++latestStarts.current;
    api.listDigestStarts(period, topicId)
      .then((list) => {
        if (latestStarts.current !== mine) return;
        setStarts({ key: startsKey, list });
        setStartsError(null);
      })
      .catch((e) => {
        if (latestStarts.current !== mine) return;
        setStarts({ key: startsKey, list: [] });
        setStartsError(errorMessage(e));
      });
  }, [period, topicId, startsKey]);

  useEffect(() => {
    loadStarts();
    return () => { latestStarts.current++; };
  }, [loadStarts, refreshTick]);

  // 条件 (日付を含む) が変わったときと refreshTick が増えたときに取り直す。
  // 日付の変化は refreshTick と同じ描画で届くので、一つの effect にまとめて二重に取らない。
  useEffect(() => {
    loadStats();
    return () => { latestStats.current++; };
  }, [loadStats, refreshTick]);

  useEffect(() => {
    setError(null);
    load(false);
    // アンマウント後や条件変更後に届いた応答を捨てる。
    return () => { latestLoad.current++; };
  }, [load, refreshTick]);
  // status は prop で届く。再取得は完了通知 (digests-changed) と記事の増減 (items-changed) のとき。
  useTauriEvent(EVENTS.digests, () => { loadStarts(); load(true); });
  // 収集で item_count が変わる。生成ジョブの完了ではないので、依頼中の印は外さない。
  useTauriEvent(EVENTS.items, () => { load(false); loadStats(); });
  useEffect(() => {
    if (!actions) return;
    actions.current = {
      periodDay: () => { setPeriod("day"); setPinned(null); },
      periodWeek: () => { setPeriod("week"); setPinned(null); },
      periodMonth: () => { setPeriod("month"); setPinned(null); },
    };
    return () => { actions.current = {}; };
  }, [actions]);

  const fail = (e: unknown) => setError(errorMessage(e));
  const request = (scopes: DigestScope[]) => {
    if (scopes.length === 0 || date === null) return;
    const keys = scopes.map(scopeKey);
    const at = context;
    setRequests((prev) => ({ context: at, keys: new Set([...(prev.context === at ? prev.keys : []), ...keys]) }));
    api.regenerateDigest(period, date, scopes).catch((e) => {
      fail(e);
      // ジョブは始まっていない。完了通知は来ないので、ここで印を外す。
      setRequests((prev) => prev.context === at
        ? { context: at, keys: new Set([...prev.keys].filter((k) => !keys.includes(k))) }
        : prev);
    });
  };

  const step = (delta: number) => {
    const next = walk[index + delta];
    if (index < 0 || next === undefined) return;
    // 最新の版に戻ったら、また新しい版に追従する。
    const newest = index + delta === walk.length - 1 && pickStart(stored ?? [], current, null) === next;
    setPinned(newest ? null : { period, start: next });
  };
  // 今日のタブでは、保存済みの日次要約を版として見せる (「11月2日 夕刊」)。
  const edition = period === "day" && date !== null && views !== null ? editionOf(date, views.map((v) => v.digest)) : null;
  const heading = edition ? editionHeading(edition, today) : periodHeading(period, date === null ? today : parseLocalDate(date), today);
  const canGenerate = !status.reader_mode;
  const shown = (views ?? []).filter((v) => sameScope(v.scope, selection));
  const scopeNote = selection.kind === "topic" ? topics.find((t) => t.id === selection.topicId)?.name ?? null : null;

  return (
    <>
      <div className="toolbar">
        <ViewSwitch view={view} onChange={onSwitchView} />
        <div className="spacer" />
        <Segmented options={PERIODS} value={period} onChange={(p) => { setPeriod(p); setPinned(null); }} ariaLabel="期間" />
        <ChatToggle open={chatOpen} onToggle={onToggleChat} />
      </div>
      <div className="view">
        <ErrorLine message={error} />
        <ErrorLine message={startsError} />
        <div className="view-header">
          <button className="btn btn-ghost" aria-label="前へ" disabled={index <= 0} onClick={() => step(-1)}><ChevronLeft size={14} /></button>
          <h1>{heading.title}</h1>
          <span className="text-secondary">{heading.range}</span>
          <button className="btn btn-ghost" aria-label="次へ" disabled={index < 0 || index >= walk.length - 1} onClick={() => step(1)}><ChevronRight size={14} /></button>
          {scopeNote && <span className="text-secondary">· {scopeNote}</span>}
          <div className="spacer" />
          {canGenerate && (
            <button className="btn btn-ghost" disabled={status.digesting || requested.size > 0 || shown.length === 0}
              onClick={() => request(shown.map((v) => v.scope))}>
              <RotateCw size={14} /> 再生成
            </button>
          )}
        </div>
        <Overview stats={stats && stats.key === loadKey ? stats.value : null} error={stats && stats.key === loadKey ? stats.error : null} />
        {views === null && !error && (
          <div className="state-box"><Loader size={15} className="spin" /><span className="grow">読み込み中…</span></div>
        )}
        {shown.map((v) => {
          const key = scopeKey(v.scope);
          const display = digestDisplay(v, status, requested.has(key));
          return (
            <section key={key} className="digest-section">
              <div className="digest-head">
                <h2>{v.label}</h2>
                {display.kind === "done" && (
                  <span className="text-tertiary tiny">{display.digest.provider} · 記事{display.digest.item_ids.length}件</span>
                )}
                {display.kind === "done" && display.regenerating && <span className="text-tertiary tiny">再生成中…</span>}
                {display.kind === "done" && (
                  <button className="row-actions-btn" aria-label={`${v.label}の要約についてチャットで質問`}
                    onClick={() => onAskChat({ type: "digest", period, date: date ?? current, scope: "category" in v.scope ? `category:${v.scope.category}` : `topic:${v.scope.topic}`, label: v.label })}>
                    <MessageSquarePlus size={14} />
                  </button>
                )}
              </div>
              {display.kind === "done" && (() => {
                const { headline, bullets, paragraphs } = parseDigestBody(display.digest.body);
                return (
                  <>
                    {headline && <p className="digest-headline">{headline}</p>}
                    {bullets.length > 0 && <ol className="digest-points">{bullets.map((b, i) => <li key={i}>{b}</li>)}</ol>}
                    {paragraphs.map((p, i) => <p key={i} style={{ margin: 0, lineHeight: 1.6 }}>{p}</p>)}
                    <References ids={display.digest.item_ids} onOpen={(id) => { api.openItem(id).catch(fail); }} />
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
                  {canGenerate && <button className="btn" onClick={() => request([v.scope])}>今すぐ再試行</button>}
                </div>
              )}
              {display.kind === "failed" && (
                <div className="state-box">
                  <CircleAlert size={15} style={{ color: "var(--danger)" }} />
                  <span className="grow">生成に{display.attempts}回失敗しました。LLMの設定を確認してから再試行してください。{display.error ? ` 最後のエラー: ${display.error}` : ""}</span>
                  {canGenerate && <button className="btn" onClick={() => request([v.scope])}>再試行</button>}
                </div>
              )}
              {display.kind === "none" && display.itemCount === 0 && (
                <div className="state-box"><Inbox size={15} /><span className="grow">この期間に記事がありません。</span></div>
              )}
              {display.kind === "none" && display.itemCount > 0 && (
                <div className="state-box">
                  <Inbox size={15} />
                  <span className="grow">まだ要約がありません (記事 {display.itemCount}件)</span>
                  {canGenerate && <button className="btn" onClick={() => request([v.scope])}>生成</button>}
                </div>
              )}
            </section>
          );
        })}
      </div>
    </>
  );
}
