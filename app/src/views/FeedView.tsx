import { Newspaper, Search, Star } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { api, EVENTS } from "../api";
import { ErrorLine } from "../components/ErrorLine";
import { Segmented, ViewSwitch } from "../components/Segmented";
import type { Selection, View } from "../components/Sidebar";
import { useTauriEvent } from "../hooks/useTauriEvent";
import { relativeTime } from "../lib/dates";
import { buildItemFilter, FEED_RANGES, type FeedRange, type FeedState } from "../lib/filters";
import { categoryLabel, errorMessage, type AppStatus, type Item, type Source, type Topic } from "../types";

interface Props {
  selection: Selection;
  topics: Topic[];
  status: AppStatus;
  view: View;
  onSwitchView: (v: View) => void;
}

function selectionTitle(selection: Selection, topics: Topic[]): string {
  if (selection.kind === "all") return "すべての記事";
  if (selection.kind === "category") return categoryLabel(selection.category);
  return topics.find((t) => t.id === selection.topicId)?.name ?? "トピック";
}

export function FeedView({ selection, topics, status, view, onSwitchView }: Props) {
  const [range, setRange] = useState<FeedRange>("week");
  const [sourceId, setSourceId] = useState<number | null>(null);
  const [unreadOnly, setUnreadOnly] = useState(false);
  const [starredOnly, setStarredOnly] = useState(false);
  const [text, setText] = useState("");
  const [items, setItems] = useState<Item[] | null>(null);
  const [sources, setSources] = useState<Source[]>([]);
  const [error, setError] = useState<string | null>(null);

  const state: FeedState = useMemo(
    () => ({ selection, range, sourceId, unreadOnly, starredOnly, text }),
    [selection, range, sourceId, unreadOnly, starredOnly, text],
  );
  // 一度も収集していなければ初回起動の空状態、それ以外の0件は絞り込みの結果として扱う。
  const neverCollected = status.last_collect_at === null;

  const load = useCallback(() => {
    api.listItems(buildItemFilter(state, new Date()))
      .then((list) => { setItems(list); setError(null); })
      .catch((e) => setError(errorMessage(e)));
  }, [state]);

  useEffect(load, [load]);
  useEffect(() => { api.listSources().then(setSources).catch(() => setSources([])); }, []);
  useTauriEvent(EVENTS.items, load);

  const run = (p: Promise<unknown>) => p.then(load).catch((e) => setError(errorMessage(e)));
  const unread = items?.filter((i) => !i.read).length ?? 0;
  const now = new Date();

  return (
    <>
      <div className="toolbar">
        <ViewSwitch view={view} onChange={onSwitchView} />
        <div className="spacer" />
        <Segmented options={FEED_RANGES} value={range} onChange={setRange} ariaLabel="期間" />
        <select className="field" aria-label="ソース" value={sourceId ?? ""} onChange={(e) => setSourceId(e.target.value ? Number(e.target.value) : null)}>
          <option value="">すべてのソース</option>
          {sources.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
        </select>
        <button className={`btn ${unreadOnly ? "btn-primary" : ""}`} aria-pressed={unreadOnly} onClick={() => setUnreadOnly(!unreadOnly)}>未読のみ</button>
        <button className={`btn ${starredOnly ? "btn-primary" : ""}`} aria-pressed={starredOnly} onClick={() => setStarredOnly(!starredOnly)}>
          <Star size={12} /> スター
        </button>
        <label className="search">
          <Search size={14} />
          <input placeholder="検索" value={text} onChange={(e) => setText(e.target.value)} />
        </label>
      </div>
      <div className="view feed">
        <ErrorLine message={error} />
        <div className="view-header">
          <h1>{selectionTitle(selection, topics)}</h1>
          <span className="text-secondary">未読 {unread}件</span>
          <div className="spacer" />
          {items && items.length > 0 && (
            <button className="link-btn" onClick={() => run(api.markAllRead(buildItemFilter(state, new Date())))}>すべて既読にする</button>
          )}
        </div>
        {items && items.length === 0 && neverCollected && !error && (
          <div className="empty">
            <Newspaper size={40} className="text-tertiary" />
            <h2>まだ記事がありません</h2>
            <p className="text-secondary">収集を始めると、GitHub、arXiv、Hugging Face、Hacker News、国内外のtechニュース、医療介護ニュースが集まり、朝7時に日次要約が生成されます。</p>
            <div style={{ display: "flex", gap: 8 }}>
              <button className="btn btn-primary" disabled={status.collecting} onClick={() => run(api.collectNow())}>今すぐ収集</button>
            </div>
            <p className="tiny text-tertiary">収集には1〜2分かかります。進行状況はサイドバー下部に表示されます。</p>
          </div>
        )}
        {items && items.length === 0 && !neverCollected && !error && (
          <div className="state-box"><span>条件に一致する記事がありません</span></div>
        )}
        {items?.map((it) => (
          <article key={it.id} className={`item-row ${it.read ? "read" : ""}`}>
            <div className="unread">{!it.read && <i aria-label="未読" />}</div>
            <div className="content">
              <button className="title" onClick={() => run(api.openItem(it.id))}>{it.title}</button>
              <div className="meta">
                <span>{it.source_name}</span>
                <span className="sep">·</span>
                <span className="time">{relativeTime(it.published_at ?? it.fetched_at, now)}</span>
                {it.score !== null && <><span className="sep">·</span><span>{it.score}</span></>}
              </div>
              {it.ai_summary && <div className="summary">{it.ai_summary}</div>}
            </div>
            <button className="star" aria-pressed={it.starred} aria-label={it.starred ? "スターを外す" : "スターを付ける"}
              onClick={() => run(api.setStarred(it.id, !it.starred))}>
              <Star size={16} fill={it.starred ? "currentColor" : "none"} />
            </button>
          </article>
        ))}
      </div>
    </>
  );
}
