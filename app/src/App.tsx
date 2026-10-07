import { useCallback, useEffect, useState } from "react";
import { api, EVENTS } from "./api";
import { Sidebar, type Selection, type View } from "./components/Sidebar";
import { ErrorLine } from "./components/ErrorLine";
import { DigestView } from "./views/DigestView";
import { FeedView } from "./views/FeedView";
import { useTauriEvent } from "./hooks/useTauriEvent";
import { errorMessage, type AppStatus, type SidebarCounts, type Topic } from "./types";

export type { View };

const EMPTY_COUNTS: SidebarCounts = { total_unread: 0, categories: {}, topics: {} };
const IDLE_STATUS: AppStatus = {
  collecting: false, summarizing: false, digesting: false,
  last_collect_at: null, next_collect_at: null, last_error: null,
};

export default function App() {
  const [view, setView] = useState<View>("digest");
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
      .then(([c, t]) => { setCounts(c); setTopics(t); setCountsError(null); })
      .catch((e) => setCountsError(errorMessage(e)));
  }, []);

  useEffect(() => { refreshStatus(); refreshCounts(); }, [refreshStatus, refreshCounts]);
  useTauriEvent(EVENTS.status, refreshStatus);
  useTauriEvent(EVENTS.items, refreshCounts);
  useTauriEvent(EVENTS.digests, refreshCounts);
  useTauriEvent(EVENTS.openSettings, () => setView("settings"));

  const collectNow = useCallback(() => {
    api.collectNow()
      .then(() => setActionError(null))
      .catch((e) => setActionError(errorMessage(e)));
  }, []);

  const select = (s: Selection) => {
    setSelection(s);
    if (view === "settings") setView("digest");
  };

  const hasError = statusError !== null || countsError !== null || actionError !== null;

  return (
    <div className="app">
      <Sidebar selection={selection} view={view} counts={counts} topics={topics} status={status}
        onSelect={select} onOpenSettings={() => setView("settings")}
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
          <DigestView selection={selection} topics={topics} status={status} view={view} onSwitchView={setView} />
        )}
        {view === "feed" && (
          <FeedView selection={selection} topics={topics} status={status} view={view} onSwitchView={setView} />
        )}
        {view === "settings" && <div className="view">{/* Task 6 */}</div>}
      </main>
    </div>
  );
}
