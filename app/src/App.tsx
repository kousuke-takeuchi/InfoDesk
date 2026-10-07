import { useCallback, useEffect, useState } from "react";
import { api, EVENTS } from "./api";
import { Sidebar, type Selection, type View } from "./components/Sidebar";
import { ErrorLine } from "./components/ErrorLine";
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
  const [shellError, setShellError] = useState<string | null>(null);

  const refreshStatus = useCallback(() => {
    api.getStatus().then(setStatus).catch((e) => setShellError(errorMessage(e)));
  }, []);
  const refreshCounts = useCallback(() => {
    Promise.all([api.sidebarCounts(), api.listTopics()])
      .then(([c, t]) => { setCounts(c); setTopics(t); setShellError(null); })
      .catch((e) => setShellError(errorMessage(e)));
  }, []);

  useEffect(() => { refreshStatus(); refreshCounts(); }, [refreshStatus, refreshCounts]);
  useTauriEvent(EVENTS.status, refreshStatus);
  useTauriEvent(EVENTS.items, refreshCounts);
  useTauriEvent(EVENTS.digests, refreshCounts);
  useTauriEvent(EVENTS.openSettings, () => setView("settings"));

  const select = (s: Selection) => {
    setSelection(s);
    if (view === "settings") setView("digest");
  };

  return (
    <div className="app">
      <Sidebar selection={selection} view={view} counts={counts} topics={topics} status={status}
        onSelect={select} onOpenSettings={() => setView("settings")}
        onCollectNow={() => api.collectNow().catch((e) => setShellError(errorMessage(e)))} />
      <main className="main">
        <ErrorLine message={shellError} />
        <div className="view">{/* Task 4〜6 でビューを差し込む */}</div>
      </main>
    </div>
  );
}
