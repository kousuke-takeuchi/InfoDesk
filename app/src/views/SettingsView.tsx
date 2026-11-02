import { useState } from "react";
import { Segmented } from "../components/Segmented";
import { SourcesTab } from "./settings/SourcesTab";
import { TopicsTab } from "./settings/TopicsTab";
import { LlmTab } from "./settings/LlmTab";
import { ScheduleTab } from "./settings/ScheduleTab";
import { SyncTab } from "./settings/SyncTab";
import type { AppStatus } from "../types";

export type SettingsTab = "sources" | "topics" | "llm" | "schedule" | "sync";
const TABS: { id: SettingsTab; label: string }[] = [
  { id: "sources", label: "ソース" }, { id: "topics", label: "トピック" }, { id: "llm", label: "LLM" }, { id: "schedule", label: "スケジュール" }, { id: "sync", label: "同期" },
];

/** 読むだけのモードで、編集を止めるタブ。LLM はチャットに使うので止めない。 */
const LOCKED: ReadonlySet<SettingsTab> = new Set<SettingsTab>(["sources", "topics", "schedule"]);
const READER_LOCKED_NOTE = "読むだけのモードです。ソース、トピック、スケジュールはサーバーの設定が使われます。サーバーの設定は CLI で変えます。";
const READER_LLM_NOTE = "読むだけのモードです。記事の要約とダイジェストはサーバーが作ります。ここの設定は AI チャットにだけ使います。";

export function SettingsView({ status, initialTab = "sources" }: { status: AppStatus; initialTab?: SettingsTab }) {
  const [tab, setTab] = useState<SettingsTab>(initialTab);
  return (
    <>
      <div className="toolbar">
        <span style={{ fontSize: 15, fontWeight: 700 }}>設定</span>
        <Segmented options={TABS} value={tab} onChange={setTab} ariaLabel="設定のタブ" />
        <div className="spacer" />
      </div>
      <div className="view settings">
        {status.reader_mode && LOCKED.has(tab) && <p className="desc" role="note">{READER_LOCKED_NOTE}</p>}
        {status.reader_mode && tab === "llm" && <p className="desc" role="note">{READER_LLM_NOTE}</p>}
        <fieldset className="settings-lock" disabled={status.reader_mode && LOCKED.has(tab)}>
          {tab === "sources" && <SourcesTab />}
          {tab === "topics" && <TopicsTab />}
          {tab === "llm" && <LlmTab />}
          {tab === "schedule" && <ScheduleTab status={status} />}
          {tab === "sync" && <SyncTab />}
        </fieldset>
      </div>
    </>
  );
}
