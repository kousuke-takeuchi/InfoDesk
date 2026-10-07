import { useState } from "react";
import { Segmented } from "../components/Segmented";
import { SourcesTab } from "./settings/SourcesTab";
import { TopicsTab } from "./settings/TopicsTab";

export type SettingsTab = "sources" | "topics" | "llm" | "schedule";
const TABS: { id: SettingsTab; label: string }[] = [
  { id: "sources", label: "ソース" }, { id: "topics", label: "トピック" }, { id: "llm", label: "LLM" }, { id: "schedule", label: "スケジュール" },
];

export function SettingsView({ initialTab = "sources" }: { initialTab?: SettingsTab }) {
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
        {tab === "llm" && <div className="state-box">LLM 設定は次のタスクで追加</div>}
        {tab === "schedule" && <div className="state-box">スケジュール設定は次のタスクで追加</div>}
      </div>
    </>
  );
}
