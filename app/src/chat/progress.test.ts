import type { UIMessage } from "ai";
import { describe, expect, it } from "vitest";
import { announceLine, answerOutcome, answerTimes, displayPlan, formatSeconds, isPlanTool, latestPlan, statusLine, workStats } from "./progress";

const msg = (parts: unknown[], metadata?: unknown): UIMessage => ({ id: "a", role: "assistant", parts, metadata } as UIMessage);
const plan = (items: unknown, toolCallId = "p") => ({ type: "tool-update_plan", toolCallId, state: "output-available", input: { items }, output: { ok: true } });

describe("progress", () => {
  it("recognizes the plan tool on both providers", () => {
    expect(isPlanTool("update_plan")).toBe(true);
    expect(isPlanTool("mcp__infodesk__update_plan")).toBe(true);
    expect(isPlanTool("search_articles")).toBe(false);
  });

  it("returns the latest valid plan", () => {
    const m = msg([
      plan([{ title: "検索", status: "in_progress" }], "p1"),
      { type: "tool-search_articles", toolCallId: "s", state: "output-available", input: {}, output: { items: [], total: 0 } },
      plan([{ title: "検索", status: "done" }, { title: "まとめる", status: "in_progress" }], "p2"),
    ]);
    expect(latestPlan(m)).toEqual([{ title: "検索", status: "done" }, { title: "まとめる", status: "in_progress" }]);
    expect(latestPlan(msg([plan([{ title: "x", status: "doing" }])]))).toBeNull();
    expect(latestPlan(msg([{ type: "text", text: "x" }]))).toBeNull();
    const claude = msg([{ type: "dynamic-tool", toolName: "mcp__infodesk__update_plan", toolCallId: "c", state: "input-available", input: { items: [{ title: "a", status: "pending" }] } }]);
    expect(latestPlan(claude)).toEqual([{ title: "a", status: "pending" }]);
  });

  it("marks every item done once the answer is finished", () => {
    const p = [{ title: "a", status: "done" as const }, { title: "b", status: "in_progress" as const }, { title: "c", status: "pending" as const }];
    expect(displayPlan(p, true)).toEqual([{ title: "a", status: "done" }, { title: "b", status: "done" }, { title: "c", status: "done" }]);
    expect(displayPlan(p, false)).toBe(p);
    expect(p[1].status).toBe("in_progress");
  });

  it("counts steps without plan updates", () => {
    const m = msg([
      { type: "reasoning", text: "考え" },
      plan([{ title: "x", status: "done" }]),
      { type: "tool-search_articles", toolCallId: "s", state: "output-available", input: {}, output: {} },
      { type: "dynamic-tool", toolName: "mcp__infodesk__fetch_page", toolCallId: "f", state: "output-available", input: {}, output: {} },
      { type: "text", text: "答え" },
    ]);
    expect(workStats(m)).toEqual({ steps: 3, tools: 2, failed: 0 });
  });

  it("reads times from metadata", () => {
    expect(answerTimes(msg([], { startedAt: "2026-10-10T00:00:00.000Z", finishedAt: "2026-10-10T00:00:18.000Z" })))
      .toEqual({ startedAt: Date.parse("2026-10-10T00:00:00.000Z"), finishedAt: Date.parse("2026-10-10T00:00:18.000Z"), finishReason: null });
    expect(answerTimes(msg([]))).toEqual({ startedAt: null, finishedAt: null, finishReason: null });
    expect(answerTimes(msg([], { startedAt: "not a date" }))).toEqual({ startedAt: null, finishedAt: null, finishReason: null });
  });

  it("formats the status line", () => {
    expect(formatSeconds(12_400)).toBe("12秒");
    expect(formatSeconds(65_000)).toBe("1分05秒");
    expect(statusLine({ running: true, elapsedMs: 12_000, steps: 3, tools: 3 })).toBe("調査中 · 12秒 · ツール3回");
    expect(statusLine({ running: true, elapsedMs: 2_000, steps: 0, tools: 0 })).toBe("調査中 · 2秒");
    expect(statusLine({ running: true, elapsedMs: null, steps: 1, tools: 1 })).toBe("調査中 · ツール1回");
    expect(statusLine({ running: false, elapsedMs: 18_000, steps: 4, tools: 3 })).toBe("4ステップ · 18秒");
    expect(statusLine({ running: false, elapsedMs: null, steps: 4, tools: 3 })).toBe("4ステップ");
  });
});

describe("progress fix round 1", () => {
  const good = plan([{ title: "検索", status: "in_progress" }], "p1");
  const want = [{ title: "検索", status: "in_progress" }];

  it("keeps the earlier plan while a later update is streaming", () => {
    const streaming = { type: "tool-update_plan", toolCallId: "p2", state: "input-streaming", input: { items: [{ title: "検" }] } };
    expect(latestPlan(msg([good, streaming]))).toEqual(want);
    const claudeStreaming = { type: "dynamic-tool", toolName: "mcp__infodesk__update_plan", toolCallId: "c", state: "input-streaming", input: { items: [{ title: "a", status: "done" }] } };
    expect(latestPlan(msg([good, claudeStreaming]))).toEqual(want);
  });

  it("keeps the earlier plan when a later update was rejected or denied", () => {
    const nine = Array.from({ length: 9 }, (_, i) => ({ title: `t${i}`, status: "pending" }));
    const rejected = { type: "tool-update_plan", toolCallId: "p2", state: "output-error", input: { items: nine }, errorText: "too many" };
    expect(latestPlan(msg([good, rejected]))).toEqual(want);
    const rejectedValid = { type: "tool-update_plan", toolCallId: "p3", state: "output-error", input: { items: [{ title: "x", status: "done" }] }, errorText: "x" };
    expect(latestPlan(msg([good, rejectedValid]))).toEqual(want);
    const denied = { type: "tool-update_plan", toolCallId: "p4", state: "output-denied", input: { items: [{ title: "x", status: "done" }] } };
    expect(latestPlan(msg([good, denied]))).toEqual(want);
  });

  it("validates item count and titles", () => {
    const nine = Array.from({ length: 9 }, (_, i) => ({ title: `t${i}`, status: "pending" }));
    const eight = nine.slice(0, 8);
    expect(latestPlan(msg([plan(eight)]))).toEqual(eight);
    expect(latestPlan(msg([plan(nine)]))).toBeNull();
    expect(latestPlan(msg([plan([])]))).toBeNull();
    expect(latestPlan(msg([plan([{ title: "  ", status: "done" }])]))).toBeNull();
    expect(latestPlan(msg([plan([{ title: "あ".repeat(80), status: "done" }])]))).toEqual([{ title: "あ".repeat(80), status: "done" }]);
    expect(latestPlan(msg([plan([{ title: "あ".repeat(81), status: "done" }])]))).toBeNull();
    expect(latestPlan(msg([good, plan(nine, "p2")]))).toEqual(want);
  });

  it("counts only non-empty reasoning and ignores step-start", () => {
    const m = msg([
      { type: "step-start" },
      { type: "reasoning", text: "  \n" },
      { type: "reasoning", text: "" },
      { type: "reasoning" },
      { type: "reasoning", text: "考え" },
      { type: "step-start" },
      { type: "tool-search_articles", toolCallId: "s", state: "output-available", input: {}, output: {} },
    ]);
    expect(workStats(m)).toEqual({ steps: 2, tools: 1, failed: 0 });
  });

  it("pins formatting edges and partial times", () => {
    expect(formatSeconds(59_600)).toBe("1分00秒");
    expect(formatSeconds(-5_000)).toBe("0秒");
    expect(answerTimes(msg([], { startedAt: "2026-10-10T00:00:00.000Z" })))
      .toEqual({ startedAt: Date.parse("2026-10-10T00:00:00.000Z"), finishedAt: null, finishReason: null });
  });
});

describe("progress failures and announcements", () => {
  it("counts failed tools, but not failed plan updates", () => {
    const m = msg([
      { type: "tool-search_articles", toolCallId: "s1", state: "output-error", input: {}, errorText: "boom" },
      { type: "tool-search_articles", toolCallId: "s2", state: "output-denied", input: {} },
      { type: "dynamic-tool", toolName: "mcp__infodesk__fetch_page", toolCallId: "f", state: "output-available", input: {}, output: {} },
      { type: "tool-update_plan", toolCallId: "p", state: "output-error", input: {}, errorText: "bad" },
    ]);
    expect(workStats(m)).toEqual({ steps: 3, tools: 3, failed: 2 });
  });

  it("appends failures to the finished line only", () => {
    expect(statusLine({ running: false, elapsedMs: 18_000, steps: 4, tools: 3, failed: 2 })).toBe("4ステップ · 18秒 · 失敗2");
    expect(statusLine({ running: false, elapsedMs: null, steps: 4, tools: 3, failed: 1 })).toBe("4ステップ · 失敗1");
    expect(statusLine({ running: false, elapsedMs: 18_000, steps: 4, tools: 3, failed: 0 })).toBe("4ステップ · 18秒");
    expect(statusLine({ running: true, elapsedMs: 5_000, steps: 2, tools: 2, failed: 1 })).toBe("調査中 · 5秒 · ツール2回");
  });

  it("builds an announcement without the ticking seconds", () => {
    expect(announceLine({ running: true, steps: 0, tools: 0, failed: 0 })).toBe("調査中");
    expect(announceLine({ running: true, steps: 3, tools: 2, failed: 0 })).toBe("調査中 · ツール2回");
    expect(announceLine({ running: false, steps: 4, tools: 3, failed: 0 })).toBe("回答しました · 4ステップ");
    expect(announceLine({ running: false, steps: 4, tools: 3, failed: 1 })).toBe("回答しました · 4ステップ · 失敗1");
  });
});

describe("progress finish reasons", () => {
  const started = "2026-10-10T00:00:00.000Z";
  const finished = "2026-10-10T00:00:18.000Z";

  it("reads the finish reason from metadata", () => {
    expect(answerTimes(msg([], { startedAt: started, finishedAt: finished, finishReason: "tool-calls" })).finishReason).toBe("tool-calls");
    expect(answerTimes(msg([], { finishedAt: finished, finishReason: 3 })).finishReason).toBeNull();
  });

  it("treats only a stop finish as answered", () => {
    expect(answerOutcome(msg([], { startedAt: started, finishedAt: finished, finishReason: "stop" }), false)).toBe("answered");
    expect(answerOutcome(msg([], { startedAt: started, finishedAt: finished, finishReason: "tool-calls" }), false)).toBe("stopped");
    expect(answerOutcome(msg([], { startedAt: started, finishedAt: finished, finishReason: "error" }), false)).toBe("stopped");
    expect(answerOutcome(msg([], { startedAt: started, finishedAt: finished }), false)).toBe("stopped");
    expect(answerOutcome(msg([], { startedAt: started }), false)).toBe("stopped");
    expect(answerOutcome(msg([]), false)).toBe("unknown");
    expect(answerOutcome(msg([], { startedAt: started }), true)).toBe("running");
  });

  it("marks a stopped answer in the folded line and the announcement", () => {
    expect(statusLine({ running: false, elapsedMs: 18_000, steps: 20, tools: 20, failed: 1, stopped: true })).toBe("20ステップ · 18秒 · 失敗1 · 途中で停止");
    expect(statusLine({ running: false, elapsedMs: null, steps: 2, tools: 2, stopped: true })).toBe("2ステップ · 途中で停止");
    expect(announceLine({ running: false, steps: 20, tools: 20, failed: 0, stopped: true })).toBe("停止しました · 20ステップ");
    expect(announceLine({ running: false, steps: 2, tools: 2, failed: 1, stopped: true })).toBe("停止しました · 2ステップ · 失敗1");
  });

  it("announces an answer without work without a step count", () => {
    expect(announceLine({ running: false, steps: 0, tools: 0, failed: 0 })).toBe("回答しました");
    expect(announceLine({ running: false, steps: 0, tools: 0, failed: 0, stopped: true })).toBe("停止しました");
  });
});
