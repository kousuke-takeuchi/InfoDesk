import { useCallback, useEffect, useState } from "react";
import { api, EVENTS } from "./api";
import { Sidebar, type Selection, type View } from "./components/Sidebar";
import { ErrorLine } from "./components/ErrorLine";
import { DigestView } from "./views/DigestView";
import { FeedView } from "./views/FeedView";
import { SettingsView } from "./views/SettingsView";
import { useTauriEvent } from "./hooks/useTauriEvent";
import { errorMessage, type AppStatus, type SidebarCounts, type Topic } from "./types";

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
  const openSettings = useCallback(() => setInSettings(true), []);
  useTauriEvent(EVENTS.openSettings, openSettings);
  const switchView = useCallback((v: View) => {
    if (v === "settings") { setInSettings(true); return; }
    setContentView(v);
    setInSettings(false);
  }, []);

  const collectNow = useCallback(() => {
    api.collectNow()
      .then(() => setActionError(null))
      .catch((e) => setActionError(errorMessage(e)));
  }, []);

  const select = (s: Selection) => {
    setSelection(s);
    setInSettings(false);
  };

  const hasError = statusError !== null || countsError !== null || actionError !== null;

  return (
    <div className="app">
      <Sidebar selection={selection} view={view} counts={counts} topics={topics} status={status}
        onSelect={select} onOpenSettings={openSettings}
        onCollectNow={collectNow} />
      <main className="main">
        {hasError && (
          <div className="shell-errors">
            <ErrorLine message={statusError} />
            <ErrorLine message={countsError} />
            <ErrorLine message={actionError} />
          </div>
        )}
        {view === "digest" && (
          <DigestView selection={selection} topics={topics} status={status} view={view} onSwitchView={switchView} />
        )}
        {view === "feed" && (
          <FeedView selection={selection} topics={topics} status={status} view={view} onSwitchView={switchView}
            onOpenSources={openSettings} />
        )}
        {view === "settings" && <SettingsView status={status} />}
      </main>
    </div>
  );
}
