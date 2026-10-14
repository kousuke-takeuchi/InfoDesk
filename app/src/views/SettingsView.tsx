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
        {tab === "sources" && <SourcesTab />}
        {tab === "topics" && <TopicsTab />}
        {tab === "llm" && <LlmTab />}
        {tab === "schedule" && <ScheduleTab status={status} />}
        {tab === "sync" && <SyncTab />}
      </div>
    </>
  );
}
