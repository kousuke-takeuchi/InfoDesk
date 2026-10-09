import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { UIMessage } from "ai";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WorkSummary } from "./WorkSummary";

const plan = { type: "tool-update_plan", toolCallId: "p", state: "output-available", input: { items: [{ title: "記事を検索", status: "done" }, { title: "まとめる", status: "in_progress" }] }, output: { ok: true } };
const search = { type: "tool-search_articles", toolCallId: "s", state: "output-available", input: {}, output: { items: [], total: 0 } };
const msg = (parts: unknown[], metadata?: unknown): UIMessage => ({ id: "a", role: "assistant", parts, metadata } as UIMessage);

afterEach(() => vi.useRealTimers());

describe("WorkSummary", () => {
  it("shows the live status and ticks every second while running", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-10-10T00:00:05.000Z"));
    render(<WorkSummary message={msg([plan, search], { startedAt: "2026-10-10T00:00:00.000Z" })} running collapsed={false} onToggle={() => {}} />);
    expect(screen.getByText("調査中 · 5秒 · ツール1回")).toBeInTheDocument();
    act(() => { vi.advanceTimersByTime(2000); });
    expect(screen.getByText("調査中 · 7秒 · ツール1回")).toBeInTheDocument();
  });

  it("shows the latest plan as a checklist", () => {
    render(<WorkSummary message={msg([plan])} running collapsed={false} onToggle={() => {}} />);
    const list = screen.getByRole("list", { name: "作業計画" });
    expect(list).toHaveTextContent("記事を検索");
    expect(screen.getByText("記事を検索").closest("li")).toHaveAttribute("data-status", "done");
    expect(screen.getByText("まとめる").closest("li")).toHaveAttribute("data-status", "in_progress");
  });

  it("collapses into a summary button after the answer", async () => {
    const onToggle = vi.fn();
    render(<WorkSummary message={msg([{ type: "reasoning", text: "考え" }, plan, search], { startedAt: "2026-10-10T00:00:00.000Z", finishedAt: "2026-10-10T00:00:18.000Z" })} running={false} collapsed onToggle={onToggle} />);
    const btn = screen.getByRole("button", { name: "2ステップ · 18秒" });
    expect(btn).toHaveAttribute("aria-expanded", "false");
    await userEvent.setup().click(btn);
    expect(onToggle).toHaveBeenCalled();
  });

  it("omits the duration for old messages and renders nothing without work", () => {
    const { rerender, container } = render(<WorkSummary message={msg([search])} running={false} collapsed onToggle={() => {}} />);
    expect(screen.getByRole("button", { name: "1ステップ" })).toBeInTheDocument();
    rerender(<WorkSummary message={msg([{ type: "text", text: "答えだけ" }])} running={false} collapsed onToggle={() => {}} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("marks every item done once the answer finished normally", () => {
    render(<WorkSummary message={msg([plan], { startedAt: "2026-10-10T00:00:00.000Z", finishedAt: "2026-10-10T00:00:18.000Z" })} running={false} collapsed onToggle={() => {}} />);
    expect(screen.getByText("まとめる").closest("li")).toHaveAttribute("data-status", "done");
    expect(document.querySelector(".work-plan .spin")).toBeNull();
  });

  it("keeps the last state without a spinner when the answer stopped without finishing", () => {
    render(<WorkSummary message={msg([plan], { startedAt: "2026-10-10T00:00:00.000Z" })} running={false} collapsed onToggle={() => {}} />);
    expect(screen.getByText("まとめる").closest("li")).toHaveAttribute("data-status", "in_progress");
    expect(document.querySelector(".work-plan .spin")).toBeNull();
  });

  it("announces only when the tool count changes, while the visible line keeps ticking", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-10-10T00:00:05.000Z"));
    const meta = { startedAt: "2026-10-10T00:00:00.000Z" };
    const { rerender } = render(<WorkSummary message={msg([plan, search], meta)} running collapsed={false} onToggle={() => {}} />);
    const status = screen.getByRole("status");
    expect(status).toHaveClass("visually-hidden");
    expect(status).toHaveTextContent("調査中 · ツール1回");
    expect(screen.getByText("調査中 · 5秒 · ツール1回").closest("[role=status]")).toBeNull();
    act(() => { vi.advanceTimersByTime(4000); });
    expect(screen.getByText("調査中 · 9秒 · ツール1回")).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent(/^調査中 · ツール1回$/);
    rerender(<WorkSummary message={msg([plan, search, { ...search, toolCallId: "s2" }], meta)} running collapsed={false} onToggle={() => {}} />);
    expect(screen.getByRole("status")).toHaveTextContent(/^調査中 · ツール2回$/);
    rerender(<WorkSummary message={msg([plan, search, { ...search, toolCallId: "s2" }], { ...meta, finishedAt: "2026-10-10T00:00:12.000Z" })} running={false} collapsed onToggle={() => {}} />);
    expect(screen.getByRole("status")).toHaveTextContent(/^回答しました · 2ステップ$/);
  });

  it("keeps failed tools visible in the folded summary", () => {
    const failed = { type: "tool-search_articles", toolCallId: "e", state: "output-error", input: {}, errorText: "boom" };
    render(<WorkSummary message={msg([search, failed], { startedAt: "2026-10-10T00:00:00.000Z", finishedAt: "2026-10-10T00:00:18.000Z" })} running={false} collapsed onToggle={() => {}} />);
    expect(screen.getByRole("button", { name: "2ステップ · 18秒 · 失敗1" })).toBeInTheDocument();
  });
});
