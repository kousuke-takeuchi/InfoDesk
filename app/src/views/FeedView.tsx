import { MessageSquarePlus, Newspaper, Search, Star } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, EVENTS } from "../api";
import { ChatToggle } from "../components/ChatToggle";
import { ErrorLine } from "../components/ErrorLine";
import { Segmented, ViewSwitch } from "../components/Segmented";
import type { Selection, View } from "../components/Sidebar";
import { useTauriEvent } from "../hooks/useTauriEvent";
import { relativeTime } from "../lib/dates";
import { buildItemFilter, FEED_RANGES, type FeedRange, type FeedState } from "../lib/filters";
import type { ChatAttachment, ChatPeriod } from "../chat/context";
import { categoryLabel, errorMessage, type AppStatus, type Item, type Source, type Topic } from "../types";

interface Props {
  selection: Selection;
  topics: Topic[];
  status: AppStatus;
  view: View;
  onSwitchView: (v: View) => void;
  /** 設定のソース一覧を開く。 */
  onOpenSources: () => void;
  chatOpen: boolean;
  onToggleChat: () => void;
  onAskChat: (a: ChatAttachment) => void;
  onPeriodChange: (p: ChatPeriod) => void;
}

const PAGE_SIZE = 100;
const SEARCH_DEBOUNCE_MS = 200;

function selectionTitle(selection: Selection, topics: Topic[]): string {
  if (selection.kind === "all") return "すべての記事";
  if (selection.kind === "category") return categoryLabel(selection.category);
  return topics.find((t) => t.id === selection.topicId)?.name ?? "トピック";
}

export function FeedView({ selection, topics, status, view, onSwitchView, onOpenSources, chatOpen, onToggleChat, onAskChat, onPeriodChange }: Props) {
  const [range, setRange] = useState<FeedRange>("week");
  const [sourceId, setSourceId] = useState<number | null>(null);
  const [unreadOnly, setUnreadOnly] = useState(false);
  const [starredOnly, setStarredOnly] = useState(false);
  // input は入力欄の表示、query は変換確定済みの文字列、text はデバウンス後に問い合わせへ使う文字列。
  const [input, setInput] = useState("");
  const [query, setQuery] = useState("");
  const [text, setText] = useState("");
  const [limit, setLimit] = useState(PAGE_SIZE);
  const [items, setItems] = useState<Item[] | null>(null);
  // 件数はバックエンドの値だけを使う。未取得・取得失敗のあいだは null のままにして何も表示しない。
  const [total, setTotal] = useState<number | null>(null);
  const [unreadTotal, setUnreadTotal] = useState<number | null>(null);
  const [sources, setSources] = useState<Source[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  // 親が毎回新しい関数を渡しても、期間が変わったときだけ通知する。
  const periodChange = useRef(onPeriodChange);
  periodChange.current = onPeriodChange;
  useEffect(() => { periodChange.current({ view: "feed", range }); }, [range]);
  const composing = useRef(false);
  const requestSeq = useRef(0);

  const state: FeedState = useMemo(
    () => ({ selection, range, sourceId, unreadOnly, starredOnly, text }),
    [selection, range, sourceId, unreadOnly, starredOnly, text],
  );
  // 選択や絞り込みが変わったら、同じレンダー内で表示件数を先頭ページへ戻す (古い limit での問い合わせを出さない)。
  const filterKey = JSON.stringify(state);
  const [limitKey, setLimitKey] = useState(filterKey);
  if (limitKey !== filterKey) {
    setLimitKey(filterKey);
    setLimit(PAGE_SIZE);
  }
  // 選択が変わったら、同じレンダー内で前の選択の行と件数を消す (新しい見出しの下に前の記事を残さない)。
  // 同じ選択の中で絞り込みを変えたときは、応答が届くまで行を残す。
  const selectionKey = JSON.stringify(selection);
  const [shownSelection, setShownSelection] = useState(selectionKey);
  if (shownSelection !== selectionKey) {
    setShownSelection(selectionKey);
    setItems(null);
    setTotal(null);
    setUnreadTotal(null);
  }
  // 一度も収集していなければ初回起動の空状態、それ以外の0件は絞り込みの結果として扱う。
  const neverCollected = status.last_collect_at === null;

  useEffect(() => {
    const timer = setTimeout(() => setText(query), SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [query]);

  const load = useCallback(() => {
    // 最後に出した問い合わせの応答だけを反映する。追い越された応答は捨てる。
    const seq = ++requestSeq.current;
    const filter = buildItemFilter(state, new Date(), limit);
    Promise.all([
      api.listItems(filter),
      api.countItems(filter),
      api.countItems({ ...filter, unread_only: true }),
    ]).then(([list, count, unreadCount]) => {
      if (seq !== requestSeq.current) return;
      setItems(list);
      setTotal(count);
      setUnreadTotal(unreadCount);
      setLoadError(null);
    }).catch((e) => {
      if (seq !== requestSeq.current) return;
      setTotal(null);
      setUnreadTotal(null);
      setLoadError(errorMessage(e));
    });
  }, [state, limit]);

  useEffect(() => {
    load();
    // アンマウント後や次の問い合わせの前に届いた応答を無効にする
    return () => { requestSeq.current += 1; };
  }, [load]);
  useEffect(() => { api.listSources().then(setSources).catch(() => setSources([])); }, []);
  useTauriEvent(EVENTS.items, load);

  // 操作後の再取得はバックエンドが出す items-changed に任せる。
  const run = (p: Promise<unknown>) => {
    p.then(() => setActionError(null)).catch((e) => setActionError(errorMessage(e)));
  };
  const now = new Date();
  const shown = items?.length ?? 0;

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
          <input
            placeholder="検索"
            aria-label="検索"
            value={input}
            onChange={(e) => {
              setInput(e.target.value);
              // 変換中の文字列では検索しない。確定時に onCompositionEnd で反映する。
              if (!composing.current) setQuery(e.target.value);
            }}
            onCompositionStart={() => { composing.current = true; }}
            onCompositionEnd={(e) => {
              composing.current = false;
              setQuery(e.currentTarget.value);
            }}
          />
        </label>
        <ChatToggle open={chatOpen} onToggle={onToggleChat} />
      </div>
      <div className="view feed">
        <ErrorLine message={loadError} />
        <ErrorLine message={actionError} />
        <div className="view-header">
          <h1>{selectionTitle(selection, topics)}</h1>
          {unreadTotal !== null && <span className="text-secondary">未読 {unreadTotal}件</span>}
          <div className="spacer" />
          {unreadTotal !== null && unreadTotal > 0 && (
            <button className="link-btn" onClick={() => run(api.markAllRead(buildItemFilter(state, new Date())))}>すべて既読にする</button>
          )}
        </div>
        {items && items.length === 0 && neverCollected && !loadError && (
          <div className="empty">
            <Newspaper size={40} className="text-tertiary" />
            <h2>まだ記事がありません</h2>
            <p className="text-secondary">収集を始めると、GitHub、arXiv、Hugging Face、Hacker News、国内外のtechニュース、医療介護ニュースが集まり、朝7時に日次要約が生成されます。</p>
            <div style={{ display: "flex", gap: 8 }}>
              <button className="btn btn-primary" disabled={status.collecting} onClick={() => run(api.collectNow())}>今すぐ収集</button>
              <button className="btn" onClick={onOpenSources}>ソースを確認</button>
            </div>
            <p className="tiny text-tertiary">収集には1〜2分かかります。進行状況はサイドバー下部に表示されます。</p>
          </div>
        )}
        {items && items.length === 0 && !neverCollected && !loadError && (
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
            <button className="row-actions" aria-label={`${it.title} についてチャットで質問`}
              onClick={(e) => { e.stopPropagation(); onAskChat({ type: "item", id: it.id, title: it.title }); }}>
              <MessageSquarePlus size={16} />
            </button>
            <button className="star" aria-pressed={it.starred} aria-label={it.starred ? "スターを外す" : "スターを付ける"}
              onClick={() => run(api.setStarred(it.id, !it.starred))}>
              <Star size={16} fill={it.starred ? "currentColor" : "none"} />
            </button>
          </article>
        ))}
        {total !== null && total > shown && (
          <div className="feed-footer">
            <span className="text-secondary">{total}件中 {shown}件を表示</span>
            <button className="btn" onClick={() => setLimit(limit + PAGE_SIZE)}>さらに表示</button>
          </div>
        )}
      </div>
    </>
  );
}
