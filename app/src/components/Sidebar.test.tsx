import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Sidebar } from "./Sidebar";
import type { AppStatus, SidebarCounts, Topic } from "../types";

const counts: SidebarCounts = {
  total_unread: 128,
  categories: { github: 25, arxiv: 42, huggingface: 18, hackernews: 30, tech_jp: 21, tech_intl: 16, medcare: 14 },
  topics: { "1": 6, "2": 9 },
};
const topics: Topic[] = [
  { id: 1, name: "認知症", keywords: ["認知症"] },
  { id: 2, name: "介護", keywords: ["介護"] },
];
const status: AppStatus = {
  collecting: false, summarizing: false, digesting: false,
  last_collect_at: "2026-10-06T00:00:00Z", next_collect_at: null, last_error: null,
};

describe("Sidebar", () => {
  it("lists categories and topics with unread counts and marks the selection", () => {
    render(
      <Sidebar selection={{ kind: "category", category: "medcare" }} view="feed" counts={counts} topics={topics}
        status={status} onSelect={() => {}} onOpenSettings={() => {}} onCollectNow={() => {}} />,
    );
    expect(screen.getByRole("button", { name: /すべて.*128/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /医療介護.*14/ })).toHaveAttribute("aria-current", "true");
    expect(screen.getByRole("button", { name: /GitHub.*25/ })).not.toHaveAttribute("aria-current", "true");
    expect(screen.getByRole("button", { name: /認知症.*6/ })).toBeInTheDocument();
    expect(screen.getByText(/最終収集 /)).toBeInTheDocument();
  });

  it("hides zero counts, shows collecting state, and reports selection", async () => {
    const onSelect = vi.fn();
    const onOpenSettings = vi.fn();
    render(
      <Sidebar selection={{ kind: "all" }} view="digest"
        counts={{ total_unread: 0, categories: {}, topics: {} }} topics={topics}
        status={{ ...status, collecting: true }} onSelect={onSelect} onOpenSettings={onOpenSettings} onCollectNow={() => {}} />,
    );
    expect(screen.getByRole("button", { name: "すべて" })).toHaveAttribute("aria-current", "true");
    expect(screen.queryByText("0")).not.toBeInTheDocument();
    expect(screen.getByText("収集中…")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "arXiv" }));
    expect(onSelect).toHaveBeenCalledWith({ kind: "category", category: "arxiv" });
    await userEvent.click(screen.getByRole("button", { name: "介護" }));
    expect(onSelect).toHaveBeenCalledWith({ kind: "topic", topicId: 2 });
    await userEvent.click(screen.getByRole("button", { name: "設定" }));
    expect(onOpenSettings).toHaveBeenCalled();
  });

  it("shows the last error and the settings item as current on the settings view", () => {
    render(
      <Sidebar selection={{ kind: "all" }} view="settings" counts={counts} topics={[]}
        status={{ ...status, last_error: "ollama 404: model not found" }}
        onSelect={() => {}} onOpenSettings={() => {}} onCollectNow={() => {}} />,
    );
    expect(screen.getByRole("button", { name: "設定" })).toHaveAttribute("aria-current", "true");
    expect(screen.getByText(/ollama 404/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /すべて.*128/ })).not.toHaveAttribute("aria-current", "true");
  });
});
