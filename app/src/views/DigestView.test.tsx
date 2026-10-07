import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { DigestView } from "./DigestView";
import { EVENTS } from "../api";
import { emitEvent, setupIpc } from "../test/ipc";
import type { AppStatus, Digest, DigestView as DV } from "../types";

const status: AppStatus = { collecting: false, summarizing: false, digesting: false, last_collect_at: null, next_collect_at: null, last_error: null };
const digest = (over: Partial<Digest>): Digest => ({
  id: 1, period: "day", period_start: "2026-10-06", scope: { category: "github" }, body: "- 一つ目。\n- 二つ目。\n全体の傾向。",
  item_ids: [11, 12], provider: "ollama:gemma4:26b", status: "done", attempts: 0, created_at: "2026-10-06T00:00:00Z", last_error: null, ...over,
});
const views: DV[] = [
  { scope: { category: "github" }, label: "GitHub", digest: digest({}) },
  { scope: { category: "arxiv" }, label: "arXiv", digest: digest({ id: 2, scope: { category: "arxiv" }, status: "pending", attempts: 1, last_error: "ollama 500: unexpected EOF", body: "" }) },
  { scope: { category: "medcare" }, label: "医療介護", digest: digest({ id: 3, scope: { category: "medcare" }, status: "failed", attempts: 3, last_error: "ollama 404: model not found", body: "" }) },
  { scope: { category: "huggingface" }, label: "Hugging Face", digest: null },
];

beforeEach(() => { vi.useFakeTimers({ toFake: ["Date"] }); vi.setSystemTime(new Date(2026, 9, 6, 9, 0)); });
afterEach(() => vi.useRealTimers());

describe("DigestView", () => {
  it("renders sections per state and the heading for today", async () => {
    // Review Focus 3
    const ipc = setupIpc({
      get_digests: () => views,
      get_item: (a) => ({ id: a.id, title: `記事${a.id}`, url: "https://e.com", source_name: "s", category: "github", excerpt: "", author: null, published_at: null, fetched_at: "2026-10-06T00:00:00Z", score: null, ai_summary: null, read: false, starred: false, source_id: 1 }),
      regenerate_digest: () => undefined, retry_pending_digests: () => undefined, open_item: () => undefined,
    });
    render(<DigestView selection={{ kind: "all" }} topics={[]} status={status} view="digest" onSwitchView={() => {}} />);
    expect(await screen.findByRole("heading", { name: "今日のダイジェスト" })).toBeInTheDocument();
    expect(screen.getByText("2026年10月6日(火)")).toBeInTheDocument();
    expect(ipc.last("get_digests")).toEqual({ period: "day", date: "2026-10-06", topicId: null });

    expect(screen.getByText("一つ目。")).toBeInTheDocument();
    expect(screen.getByText("全体の傾向。")).toBeInTheDocument();
    expect(screen.getByText(/ollama:gemma4:26b · 記事2件/)).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "記事11" })).toBeInTheDocument();

    expect(screen.getByText(/保留中/)).toHaveTextContent("ollama 500: unexpected EOF");
    expect(screen.getByText(/3回失敗/)).toHaveTextContent("ollama 404: model not found");
    expect(screen.getByText("この期間に記事がありません。")).toBeInTheDocument();

    await userEvent.click(screen.getAllByRole("button", { name: "今すぐ再試行" })[0]);
    expect(ipc.last("regenerate_digest")).toEqual({ period: "day", date: "2026-10-06", scope: { category: "arxiv" } });
  });

  it("switches period and scope and shows generating", async () => {
    const ipc = setupIpc({ get_digests: () => [{ scope: { topic: 2 }, label: "介護", digest: null }], get_item: () => null });
    render(<DigestView selection={{ kind: "topic", topicId: 2 }} topics={[{ id: 2, name: "介護", keywords: [] }]}
      status={{ ...status, digesting: true }} view="digest" onSwitchView={() => {}} />);
    expect(await screen.findByText("要約を生成中…")).toBeInTheDocument();
    expect(ipc.last("get_digests")).toEqual({ period: "day", date: "2026-10-06", topicId: 2 });

    await userEvent.click(screen.getByRole("button", { name: "週" }));
    expect(await screen.findByRole("heading", { name: "今週のダイジェスト" })).toBeInTheDocument();
    expect(screen.getByText("2026年10月5日(月) 〜 10月11日(日)")).toBeInTheDocument();
    expect(ipc.last("get_digests")).toEqual({ period: "week", date: "2026-10-06", topicId: 2 });
  });

  it("filters to one category when a category is selected and reports errors", async () => {
    setupIpc({ get_digests: () => views, get_item: () => null });
    render(<DigestView selection={{ kind: "category", category: "medcare" }} topics={[]} status={status} view="digest" onSwitchView={() => {}} />);
    expect(await screen.findByRole("heading", { name: "医療介護" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "今日のダイジェスト" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "GitHub" })).not.toBeInTheDocument();

    setupIpc({ get_digests: () => { throw { message: "unknown period 'year'" }; } });
    render(<DigestView selection={{ kind: "all" }} topics={[]} status={status} view="digest" onSwitchView={() => {}} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("unknown period 'year'");
  });

  it("refetches digests when digests-changed fires", async () => {
    const ipc = setupIpc({ get_digests: () => views, get_item: () => null });
    render(<DigestView selection={{ kind: "all" }} topics={[]} status={status} view="digest" onSwitchView={() => {}} />);
    expect(await screen.findByRole("heading", { name: "今日のダイジェスト" })).toBeInTheDocument();
    await waitFor(() => expect(ipc.count("get_digests")).toBe(1));
    await emitEvent(EVENTS.digests);
    await waitFor(() => expect(ipc.count("get_digests")).toBe(2));
  });
});
