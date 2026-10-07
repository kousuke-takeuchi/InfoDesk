import { describe, expect, it } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import App from "./App";
import { EVENTS } from "./api";
import { emitEvent, setupIpc, type IpcHandlers } from "./test/ipc";
import type { AppStatus, SidebarCounts, Topic } from "./types";

const STATUS: AppStatus = {
  collecting: false, summarizing: false, digesting: false,
  last_collect_at: null, next_collect_at: null, last_error: null,
};
const TOPICS: Topic[] = [];

function counts(total: number): SidebarCounts {
  return { total_unread: total, categories: {}, topics: {} };
}

function handlers(over: IpcHandlers = {}): IpcHandlers {
  return {
    get_status: () => STATUS,
    sidebar_counts: () => counts(7),
    list_topics: () => TOPICS,
    collect_now: () => undefined,
    ...over,
  };
}

const allItem = () => screen.getByRole("button", { name: /すべて/ });

describe("App", () => {
  it("loads status, counts, and topics on mount and renders the counts", async () => {
    const ipc = setupIpc(handlers());
    render(<App />);
    await waitFor(() => expect(allItem()).toHaveTextContent("7"));
    expect(ipc.count("get_status")).toBe(1);
    expect(ipc.count("sidebar_counts")).toBe(1);
    expect(ipc.count("list_topics")).toBe(1);
    expect(ipc.unmocked).toEqual([]);
  });

  it("refetches counts when items-changed fires", async () => {
    let total = 7;
    const ipc = setupIpc(handlers({ sidebar_counts: () => counts(total) }));
    render(<App />);
    await waitFor(() => expect(allItem()).toHaveTextContent("7"));

    total = 12;
    await emitEvent(EVENTS.items);
    await waitFor(() => expect(allItem()).toHaveTextContent("12"));
    expect(ipc.count("sidebar_counts")).toBe(2);
  });

  it("refetches status when status-changed fires", async () => {
    let status = STATUS;
    const ipc = setupIpc(handlers({ get_status: () => status }));
    render(<App />);
    await waitFor(() => expect(ipc.count("get_status")).toBe(1));
    expect(await screen.findByText("未収集")).toBeInTheDocument();

    status = { ...STATUS, collecting: true };
    await emitEvent(EVENTS.status);
    expect(await screen.findByText("収集中…")).toBeInTheDocument();
    expect(ipc.count("get_status")).toBe(2);
  });

  it("switches to settings when open-settings fires", async () => {
    setupIpc(handlers());
    render(<App />);
    await waitFor(() => expect(allItem()).toHaveTextContent("7"));
    const settings = screen.getByRole("button", { name: "設定" });
    expect(settings).not.toHaveAttribute("aria-current");

    await emitEvent(EVENTS.openSettings);
    await waitFor(() => expect(settings).toHaveAttribute("aria-current", "true"));
  });

  it("keeps a get_status error when counts later succeed", async () => {
    const ipc = setupIpc(handlers({
      get_status: () => { throw { kind: "internal", message: "status が取得できません" }; },
    }));
    render(<App />);
    expect(await screen.findByRole("alert")).toHaveTextContent("status が取得できません");
    await waitFor(() => expect(allItem()).toHaveTextContent("7"));

    await emitEvent(EVENTS.items);
    await waitFor(() => expect(ipc.count("sidebar_counts")).toBe(2));
    await waitFor(() => expect(ipc.count("list_topics")).toBe(2));
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByRole("alert")).toHaveTextContent("status が取得できません");
  });

  it("shows a collect_now failure", async () => {
    setupIpc(handlers({
      collect_now: () => { throw { kind: "internal", message: "収集を開始できません" }; },
    }));
    render(<App />);
    await waitFor(() => expect(allItem()).toHaveTextContent("7"));
    expect(screen.queryByRole("alert")).toBeNull();

    await userEvent.click(screen.getByTitle("今すぐ収集"));
    expect(await screen.findByRole("alert")).toHaveTextContent("収集を開始できません");
  });
});
