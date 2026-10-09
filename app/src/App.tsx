import { useCallback, useEffect, useState } from "react";
import { api, EVENTS } from "./api";
import { Sidebar, type Selection, type View } from "./components/Sidebar";
import { ErrorLine } from "./components/ErrorLine";
import { DigestView } from "./views/DigestView";
import { FeedView } from "./views/FeedView";
import { SettingsView, type SettingsTab } from "./views/SettingsView";
import { ChatPanel } from "./chat/ChatPanel";
import type { ChatAttachment, ChatPeriod, ChatScope } from "./chat/context";
import { useTauriEvent } from "./hooks/useTauriEvent";
import { categoryLabel, errorMessage, type AppStatus, type SidebarCounts, type Topic } from "./types";

function sameAttachment(a: ChatAttachment, b: ChatAttachment): boolean {
  if (a.type === "item") return b.type === "item" && a.id === b.id;
  return b.type === "digest" && a.period === b.period && a.date === b.date && a.scope === b.scope;
}

const EMPTY_COUNTS: SidebarCounts = { total_unread: 0, categories: {}, topics: {} };
const IDLE_STATUS: AppStatus = {
  collecting: false, summarizing: false, digesting: false,
  last_collect_at: null, next_collect_at: null, last_error: null,
};

export default function App() {
  // 設定を開いているあいだも、その前に見ていた表示を覚えておき、サイドバーの項目を選んだらそこへ戻る。
  const [contentView, setContentView] = useState<Exclude<View, "settings">>("digest");
  const [inSettings, setInSettings] = useState(false);
  const view: View = inSettings ? "settings" : contentView;
  const [selection, setSelection] = useState<Selection>({ kind: "all" });
  const [status, setStatus] = useState<AppStatus>(IDLE_STATUS);
  const [counts, setCounts] = useState<SidebarCounts>(EMPTY_COUNTS);
  const [topics, setTopics] = useState<Topic[]>([]);
  // エラーは発生源ごとに持つ。ある取得の成功が別の発生源のエラーを消さないようにする。
  const [statusError, setStatusError] = useState<string | null>(null);
  const [countsError, setCountsError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [chatOpen, setChatOpen] = useState(false);
  const [chatPeriod, setChatPeriod] = useState<ChatPeriod | null>(null);
  const [attachments, setAttachments] = useState<ChatAttachment[]>([]);
  // 選択が変わったら添付は引き継がない (トピックの削除による選択の巻き戻しも含む)。
  const selectionId = JSON.stringify(selection);
  const [attachedFor, setAttachedFor] = useState(selectionId);
  if (attachedFor !== selectionId) {
    setAttachedFor(selectionId);
    setAttachments([]);
  }

  const refreshStatus = useCallback(() => {
    api.getStatus()
      .then((s) => { setStatus(s); setStatusError(null); })
      .catch((e) => setStatusError(errorMessage(e)));
  }, []);
  const refreshCounts = useCallback(() => {
    Promise.all([api.sidebarCounts(), api.listTopics()])
      .then(([c, t]) => {
        setCounts(c); setTopics(t); setCountsError(null);
        // 選択中のトピックが削除されていたら、存在しない選択を残さず「すべて」に戻す。
        setSelection((cur) => (cur.kind === "topic" && !t.some((x) => x.id === cur.topicId) ? { kind: "all" } : cur));
      })
      .catch((e) => setCountsError(errorMessage(e)));
  }, []);

  useEffect(() => { refreshStatus(); refreshCounts(); }, [refreshStatus, refreshCounts]);
  useTauriEvent(EVENTS.status, refreshStatus);
  useTauriEvent(EVENTS.items, refreshCounts);
  useTauriEvent(EVENTS.digests, refreshCounts);
  // 設定を開くたびに key を増やし、initialTab を取り直す。
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("sources");
  const [settingsKey, setSettingsKey] = useState(0);
  const openSettingsAt = useCallback((tab: SettingsTab) => {
    setSettingsTab(tab);
    setSettingsKey((k) => k + 1);
    setInSettings(true);
  }, []);
  const openSettings = useCallback(() => openSettingsAt("sources"), [openSettingsAt]);
  const openLlmSettings = useCallback(() => openSettingsAt("llm"), [openSettingsAt]);
  useTauriEvent(EVENTS.openSettings, openSettings);
  const switchView = useCallback((v: View) => {
    if (v === "settings") { openSettings(); return; }
    setContentView(v);
    setInSettings(false);
  }, [openSettings]);

  const collectNow = useCallback(() => {
    api.collectNow()
      .then(() => setActionError(null))
      .catch((e) => setActionError(errorMessage(e)));
  }, []);

  const select = (s: Selection) => {
    setSelection(s);
    setInSettings(false);
  };

  const toggleChat = useCallback(() => setChatOpen((o) => !o), []);
  const askChat = useCallback((a: ChatAttachment) => {
    setAttachments((cur) => (cur.some((x) => sameAttachment(x, a)) ? cur : [...cur, a]));
    setChatOpen(true);
  }, []);
  const removeAttachment = useCallback((i: number) => setAttachments((cur) => cur.filter((_, j) => j !== i)), []);

  const scope: ChatScope = selection.kind === "all"
    ? { kind: "all" }
    : selection.kind === "category"
      ? { kind: "category", category: selection.category, label: categoryLabel(selection.category) }
      : { kind: "topic", topicId: selection.topicId, label: topics.find((t) => t.id === selection.topicId)?.name ?? "トピック" };

  const hasError = statusError !== null || countsError !== null || actionError !== null;

  return (
    <div className="app">
      <Sidebar selection={selection} view={view} counts={counts} topics={topics} status={status}
        onSelect={select} onOpenSettings={openSettings}
        onCollectNow={collectNow} />
      <main className="main">
        <div className="content">
          {hasError && (
            <div className="shell-errors">
              <ErrorLine message={statusError} />
              <ErrorLine message={countsError} />
              <ErrorLine message={actionError} />
            </div>
          )}
          {view === "digest" && (
            <DigestView selection={selection} topics={topics} status={status} view={view} onSwitchView={switchView}
              chatOpen={chatOpen} onToggleChat={toggleChat} onAskChat={askChat} onPeriodChange={setChatPeriod} />
          )}
          {view === "feed" && (
            <FeedView selection={selection} topics={topics} status={status} view={view} onSwitchView={switchView}
              onOpenSources={openSettings}
              chatOpen={chatOpen} onToggleChat={toggleChat} onAskChat={askChat} onPeriodChange={setChatPeriod} />
          )}
          {view === "settings" && <SettingsView key={settingsKey} status={status} initialTab={settingsTab} />}
        </div>
        {view !== "settings" && chatOpen && (
          <ChatPanel scope={scope} period={chatPeriod} attachments={attachments}
            onRemoveAttachment={removeAttachment} onClose={() => setChatOpen(false)} onOpenSettings={openLlmSettings} />
        )}
      </main>
    </div>
  );
}
