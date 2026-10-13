import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { DigestView } from "./DigestView";
import { EVENTS } from "../api";
import { emitEvent, setupIpc as baseSetupIpc, type IpcHandlers } from "../test/ipc";
import type { AppStatus, Digest, DigestStats, DigestView as DV } from "../types";

const EMPTY_STATS: DigestStats = { period: "day", period_start: "2026-10-06", total: 0, categories: [], topics: [] };
const WEEK_STATS: DigestStats = {
  period: "week", period_start: "2026-10-05", total: 7,
  categories: [{ category: "github", count: 7, daily: [1, 1, 1, 1, 1, 1, 1] }],
  topics: [{ topic_id: 1, name: "認知症", count: 3, prev_count: 1 }],
};
/** どのテストでも概観の取得が呼ばれるので、既定の digest_stats を足す。 */
const setupIpc = (handlers: IpcHandlers) => baseSetupIpc({ digest_stats: () => EMPTY_STATS, ...handlers });

const status: AppStatus = { collecting: false, summarizing: false, digesting: false, last_collect_at: null, next_collect_at: null, last_error: null };
const digest = (over: Partial<Digest>): Digest => ({
  id: 1, period: "day", period_start: "2026-10-06", scope: { category: "github" }, body: "- 一つ目。\n- 二つ目。\n全体の傾向。",
  item_ids: [11, 12], provider: "ollama:gemma4:26b", status: "done", attempts: 0, created_at: "2026-10-06T00:00:00Z", last_error: null, ...over,
});
const views: DV[] = [
  { scope: { category: "github" }, label: "GitHub", digest: digest({}), item_count: 2 },
  { scope: { category: "arxiv" }, label: "arXiv", digest: digest({ id: 2, scope: { category: "arxiv" }, status: "pending", attempts: 1, last_error: "ollama 500: unexpected EOF", body: "" }), item_count: 4 },
  { scope: { category: "medcare" }, label: "医療介護", digest: digest({ id: 3, scope: { category: "medcare" }, status: "failed", attempts: 3, last_error: "ollama 404: model not found", body: "" }), item_count: 5 },
  { scope: { category: "huggingface" }, label: "Hugging Face", digest: null, item_count: 0 },
];
const item = (a: { id: number }) => ({ id: a.id, title: `記事${a.id}`, url: "https://e.com", source_name: "s", category: "github", excerpt: "", author: null, published_at: null, fetched_at: "2026-10-06T00:00:00Z", score: null, ai_summary: null, read: false, starred: false, image_url: null, source_id: 1 });
/** バックエンドと同じく、呼び出しごとに新しいオブジェクトを返す。 */
const fresh = <T,>(v: T): T => JSON.parse(JSON.stringify(v));
const section = (name: string) => within(screen.getByRole("heading", { name }).closest("section")!);
const CHAT = { chatOpen: false, onToggleChat: () => {}, onAskChat: () => {}, onPeriodChange: () => {} };
const renderAll = (s: AppStatus = status, chat: Partial<typeof CHAT> = {}) =>
  render(<DigestView selection={{ kind: "all" }} topics={[]} status={s} view="digest" onSwitchView={() => {}} {...CHAT} {...chat} />);
/** 保留中の IPC 応答と、それに続く state 更新を流し切る。 */
const settle = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });

beforeEach(() => { vi.useFakeTimers({ toFake: ["Date"] }); vi.setSystemTime(new Date(2026, 9, 6, 9, 0)); });
afterEach(() => vi.useRealTimers());

describe("DigestView", () => {
  it("renders sections per state and the heading for today", async () => {
    // Review Focus 3
    const ipc = setupIpc({
      get_digests: () => views,
      get_item: item,
      regenerate_digest: () => undefined, retry_pending_digests: () => undefined, open_item: () => undefined,
    });
    renderAll();
    expect(await screen.findByRole("heading", { name: "今日のダイジェスト" })).toBeInTheDocument();
    expect(screen.getByText("2026年10月6日(火)")).toBeInTheDocument();
    expect(ipc.last("get_digests")).toEqual({ period: "day", date: "2026-10-06", topicId: null });

    expect(await screen.findByText("一つ目。")).toBeInTheDocument();
    expect(screen.getByText("全体の傾向。")).toBeInTheDocument();
    expect(screen.getByText(/ollama:gemma4:26b · 記事2件/)).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "記事11" })).toBeInTheDocument();

    expect(screen.getByText(/保留中/)).toHaveTextContent("ollama 500: unexpected EOF");
    expect(screen.getByText(/3回失敗/)).toHaveTextContent("ollama 404: model not found");
    expect(screen.getByText("この期間に記事がありません。")).toBeInTheDocument();

    await userEvent.click(screen.getAllByRole("button", { name: "今すぐ再試行" })[0]);
    expect(ipc.last("regenerate_digest")).toEqual({ period: "day", date: "2026-10-06", scopes: [{ category: "arxiv" }] });
  });

  it("switches period and scope and shows generating", async () => {
    const ipc = setupIpc({ get_digests: () => [{ scope: { topic: 2 }, label: "介護", digest: null, item_count: 3 }], get_item: () => null });
    render(<DigestView selection={{ kind: "topic", topicId: 2 }} topics={[{ id: 2, name: "介護", keywords: [] }]}
      status={{ ...status, digesting: true }} view="digest" onSwitchView={() => {}} {...CHAT} />);
    expect(await screen.findByText("要約を生成中…")).toBeInTheDocument();
    expect(ipc.last("get_digests")).toEqual({ period: "day", date: "2026-10-06", topicId: 2 });

    await userEvent.click(screen.getByRole("button", { name: "週" }));
    expect(await screen.findByRole("heading", { name: "今週のダイジェスト" })).toBeInTheDocument();
    expect(screen.getByText("2026年10月5日(月) 〜 10月11日(日)")).toBeInTheDocument();
    expect(ipc.last("get_digests")).toEqual({ period: "week", date: "2026-10-06", topicId: 2 });
  });

  it("filters to one category when a category is selected and reports errors", async () => {
    setupIpc({ get_digests: () => views, get_item: () => null });
    render(<DigestView selection={{ kind: "category", category: "medcare" }} topics={[]} status={status} view="digest" onSwitchView={() => {}} {...CHAT} />);
    expect(await screen.findByRole("heading", { name: "医療介護" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "今日のダイジェスト" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "GitHub" })).not.toBeInTheDocument();

    setupIpc({ get_digests: () => { throw { message: "unknown period 'year'" }; } });
    renderAll();
    expect(await screen.findByRole("alert")).toHaveTextContent("unknown period 'year'");
  });

  it("shows the period overview above the sections", async () => {
    const ipc = setupIpc({ get_digests: () => views, get_item: () => null, digest_stats: () => WEEK_STATS });
    renderAll();
    expect(await screen.findByRole("group", { name: /カテゴリー別の記事数/ })).toBeInTheDocument();
    expect(ipc.last("digest_stats")).toEqual({ period: "day", date: "2026-10-06", topicId: null });
  });

  it("refetches the overview for a new period and on items-changed", async () => {
    const ipc = setupIpc({ get_digests: () => views, get_item: () => null, digest_stats: () => WEEK_STATS });
    renderAll();
    await screen.findByRole("group", { name: /カテゴリー別の記事数/ });
    await userEvent.click(screen.getByRole("button", { name: "週" }));
    await waitFor(() => expect(ipc.last("digest_stats")).toEqual({ period: "week", date: "2026-10-06", topicId: null }));
    const before = ipc.count("digest_stats");
    await emitEvent(EVENTS.items);
    await waitFor(() => expect(ipc.count("digest_stats")).toBe(before + 1));
  });

  it("reports an overview failure without hiding the sections", async () => {
    setupIpc({ get_digests: () => views, get_item: () => null, digest_stats: () => { throw { message: "stats failed" }; } });
    renderAll();
    expect(await screen.findByText("stats failed")).toBeInTheDocument();
    expect(await screen.findByText("一つ目。")).toBeInTheDocument();
  });

  it("refetches digests when digests-changed fires", async () => {
    const ipc = setupIpc({ get_digests: () => views, get_item: () => null });
    renderAll();
    expect(await screen.findByRole("heading", { name: "今日のダイジェスト" })).toBeInTheDocument();
    await waitFor(() => expect(ipc.count("get_digests")).toBe(1));
    await emitEvent(EVENTS.digests);
    await waitFor(() => expect(ipc.count("get_digests")).toBe(2));
  });

  it("sends every shown scope in one regenerate call and locks the button until digests-changed", async () => {
    const ipc = setupIpc({ get_digests: () => fresh(views), get_item: item, regenerate_digest: () => undefined });
    renderAll();
    expect(await screen.findByText("一つ目。")).toBeInTheDocument();
    const button = screen.getByRole("button", { name: "再生成" });
    expect(button).toBeEnabled();

    await userEvent.click(button);
    expect(ipc.count("regenerate_digest")).toBe(1);
    expect(ipc.last("regenerate_digest")).toEqual({
      period: "day", date: "2026-10-06",
      scopes: [{ category: "github" }, { category: "arxiv" }, { category: "medcare" }, { category: "huggingface" }],
    });
    expect(button).toBeDisabled();
    // 生成済みの本文は残し、再生成中であることだけ添える。
    expect(section("GitHub").getByText("一つ目。")).toBeInTheDocument();
    expect(section("GitHub").getByText("再生成中…")).toBeInTheDocument();
    // 依頼した残りは生成中になり、個別のボタンは出ない。
    for (const name of ["arXiv", "医療介護", "Hugging Face"]) {
      expect(section(name).getByText("要約を生成中…")).toBeInTheDocument();
    }
    expect(screen.queryByRole("button", { name: "今すぐ再試行" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "再試行" })).not.toBeInTheDocument();

    await emitEvent(EVENTS.digests);
    await waitFor(() => expect(button).toBeEnabled());
    expect(screen.queryByText("再生成中…")).not.toBeInTheDocument();
    expect(section("医療介護").getByText(/3回失敗/)).toBeInTheDocument();
    expect(ipc.count("regenerate_digest")).toBe(1);
  });

  it("limits the scope to the selected category when regenerating", async () => {
    const ipc = setupIpc({ get_digests: () => fresh(views), get_item: () => null, regenerate_digest: () => undefined });
    render(<DigestView selection={{ kind: "category", category: "github" }} topics={[]} status={status} view="digest" onSwitchView={() => {}} {...CHAT} />);
    expect(await screen.findByText("一つ目。")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "再生成" }));
    expect(ipc.last("regenerate_digest")).toEqual({ period: "day", date: "2026-10-06", scopes: [{ category: "github" }] });
  });

  it("shows generating for the retried section only, then what the backend returns", async () => {
    let current = views;
    const ipc = setupIpc({ get_digests: () => fresh(current), get_item: () => null, regenerate_digest: () => undefined });
    renderAll();
    expect(await screen.findByText(/3回失敗/)).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "再試行" }));
    expect(ipc.last("regenerate_digest")).toEqual({ period: "day", date: "2026-10-06", scopes: [{ category: "medcare" }] });
    expect(section("医療介護").getByText("要約を生成中…")).toBeInTheDocument();
    expect(section("医療介護").queryByText(/3回失敗/)).not.toBeInTheDocument();
    expect(section("医療介護").queryByRole("button")).not.toBeInTheDocument();
    // 依頼していないセクションはそのまま。
    expect(section("arXiv").getByText(/保留中/)).toBeInTheDocument();
    expect(section("arXiv").getByRole("button", { name: "今すぐ再試行" })).toBeInTheDocument();
    expect(section("arXiv").queryByText("要約を生成中…")).not.toBeInTheDocument();
    expect(section("GitHub").queryByText("再生成中…")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "再生成" })).toBeDisabled();

    current = views.map((v) => "category" in v.scope && v.scope.category === "medcare"
      ? { ...v, digest: digest({ id: 3, scope: { category: "medcare" }, body: "- 復旧した要約。", item_ids: [] }) } : v);
    await emitEvent(EVENTS.digests);
    expect(await section("医療介護").findByText("復旧した要約。")).toBeInTheDocument();
    expect(section("医療介護").queryByText("要約を生成中…")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "再生成" })).toBeEnabled();
  });

  it("returns a retried section to failed when the backend still reports a failure", async () => {
    let current = views;
    setupIpc({ get_digests: () => fresh(current), get_item: () => null, regenerate_digest: () => undefined });
    renderAll();
    await userEvent.click(await screen.findByRole("button", { name: "再試行" }));
    expect(section("医療介護").getByText("要約を生成中…")).toBeInTheDocument();

    current = views.map((v) => "category" in v.scope && v.scope.category === "medcare"
      ? { ...v, digest: digest({ id: 3, scope: { category: "medcare" }, status: "failed", attempts: 4, last_error: "ollama 500: again", body: "" }) } : v);
    await emitEvent(EVENTS.digests);
    expect(await section("医療介護").findByText(/4回失敗/)).toHaveTextContent("ollama 500: again");
    expect(section("医療介護").getByRole("button", { name: "再試行" })).toBeInTheDocument();
  });

  it("says there are no articles only when the period has none", async () => {
    const ipc = setupIpc({
      get_digests: () => [{ scope: { category: "huggingface" }, label: "Hugging Face", digest: null, item_count: 0 }],
      regenerate_digest: () => undefined,
    });
    renderAll();
    expect(await screen.findByText("この期間に記事がありません。")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "生成" })).not.toBeInTheDocument();
    expect(screen.queryByText(/まだ要約がありません/)).not.toBeInTheDocument();
    expect(ipc.count("regenerate_digest")).toBe(0);
  });

  it("offers to generate when the period has articles but no digest", async () => {
    const ipc = setupIpc({
      get_digests: () => [
        { scope: { category: "github" }, label: "GitHub", digest: null, item_count: 0 },
        { scope: { category: "huggingface" }, label: "Hugging Face", digest: null, item_count: 12 },
      ],
      regenerate_digest: () => undefined,
    });
    renderAll();
    expect(await screen.findByText("まだ要約がありません (記事 12件)")).toBeInTheDocument();
    expect(section("Hugging Face").queryByText("この期間に記事がありません。")).not.toBeInTheDocument();
    expect(section("GitHub").getByText("この期間に記事がありません。")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "生成" }));
    expect(ipc.count("regenerate_digest")).toBe(1);
    expect(ipc.last("regenerate_digest")).toEqual({ period: "day", date: "2026-10-06", scopes: [{ category: "huggingface" }] });
    expect(section("Hugging Face").getByText("要約を生成中…")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "生成" })).not.toBeInTheDocument();
    expect(section("GitHub").getByText("この期間に記事がありません。")).toBeInTheDocument();
  });

  it("clears the old period's sections as soon as the period changes", async () => {
    let resolveWeek: (v: DV[]) => void = () => {};
    setupIpc({
      get_digests: (a) => a.period === "day" ? views : new Promise<DV[]>((r) => { resolveWeek = r; }),
      get_item: () => null,
    });
    renderAll();
    expect(await screen.findByText("一つ目。")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "週" }));
    expect(screen.getByRole("heading", { name: "今週のダイジェスト" })).toBeInTheDocument();
    // 週の応答はまだ返っていない。今日の本文を今週の見出しの下に残さない。
    expect(screen.queryByText("一つ目。")).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "GitHub" })).not.toBeInTheDocument();
    expect(screen.getByText("読み込み中…")).toBeInTheDocument();

    await act(async () => {
      resolveWeek([{ scope: { category: "github" }, label: "GitHub", digest: digest({ period: "week", body: "- 週の要点。", item_ids: [] }), item_count: 9 }]);
    });
    expect(await screen.findByText("週の要点。")).toBeInTheDocument();
    expect(screen.queryByText("読み込み中…")).not.toBeInTheDocument();
  });

  it("ignores a stale response that arrives after a newer one", async () => {
    let resolveDay: (v: DV[]) => void = () => {};
    const ipc = setupIpc({
      get_digests: (a) => a.period === "day"
        ? new Promise<DV[]>((r) => { resolveDay = r; })
        : [{ scope: { category: "github" }, label: "GitHub", digest: digest({ period: "week", body: "- 週の要点。", item_ids: [] }), item_count: 9 }],
      get_item: () => null,
    });
    renderAll();
    expect(await screen.findByText("読み込み中…")).toBeInTheDocument();
    await waitFor(() => expect(ipc.count("get_digests")).toBe(1));

    await userEvent.click(screen.getByRole("button", { name: "週" }));
    expect(await screen.findByText("週の要点。")).toBeInTheDocument();

    await act(async () => { resolveDay(views); });
    await settle();
    expect(screen.queryByText("一つ目。")).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "arXiv" })).not.toBeInTheDocument();
    expect(screen.getByText("週の要点。")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "今週のダイジェスト" })).toBeInTheDocument();
  });

  it("does not refetch digests when the status changes", async () => {
    const ipc = setupIpc({ get_digests: () => fresh(views), get_item: item });
    const { rerender } = renderAll();
    expect(await screen.findByRole("button", { name: "記事11" })).toBeInTheDocument();
    expect(ipc.count("get_digests")).toBe(1);

    rerender(<DigestView selection={{ kind: "all" }} topics={[]} status={{ ...status, digesting: true }} view="digest" onSwitchView={() => {}} {...CHAT} />);
    await emitEvent(EVENTS.status);
    await settle();
    expect(ipc.count("get_digests")).toBe(1);
    // status は prop で届くので、表示は切り替わる。
    expect(section("arXiv").getByText("要約を生成中…")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "再生成" })).toBeDisabled();
  });

  it("does not refetch reference items when a reload returns the same item ids", async () => {
    let current = views;
    const ipc = setupIpc({ get_digests: () => fresh(current), get_item: item });
    renderAll();
    expect(await screen.findByRole("button", { name: "記事12" })).toBeInTheDocument();
    expect(ipc.count("get_item")).toBe(2);

    await emitEvent(EVENTS.digests);
    await waitFor(() => expect(ipc.count("get_digests")).toBe(2));
    await settle();
    expect(ipc.count("get_item")).toBe(2);
    expect(screen.getByRole("button", { name: "記事11" })).toBeInTheDocument();

    // 根拠記事が変われば取り直す。
    current = [{ ...views[0], digest: digest({ item_ids: [12, 13] }) }, ...views.slice(1)];
    await emitEvent(EVENTS.digests);
    expect(await screen.findByRole("button", { name: "記事13" })).toBeInTheDocument();
    expect(ipc.count("get_item")).toBe(4);
    expect(screen.queryByRole("button", { name: "記事11" })).not.toBeInTheDocument();
  });

  it("shows the error and drops the requested state when the regenerate call is rejected", async () => {
    const ipc = setupIpc({
      get_digests: () => fresh(views), get_item: () => null,
      regenerate_digest: () => { throw { message: "unknown period 'day'" }; },
    });
    renderAll();
    await userEvent.click(await screen.findByRole("button", { name: "再試行" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("unknown period 'day'");
    expect(ipc.count("regenerate_digest")).toBe(1);
    await waitFor(() => expect(section("医療介護").getByText(/3回失敗/)).toBeInTheDocument());
    expect(section("医療介護").getByRole("button", { name: "再試行" })).toBeInTheDocument();
    expect(screen.queryByText("要約を生成中…")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "再生成" })).toBeEnabled();
  });

  it("omits the bullet list and the reference row when there is nothing to put in them", async () => {
    let resolveItem: (() => void) | null = null;
    setupIpc({
      get_digests: () => [{ scope: { category: "github" }, label: "GitHub", digest: digest({ body: "全体の傾向だけ。", item_ids: [11] }), item_count: 1 }],
      get_item: (a) => new Promise((r) => { resolveItem = () => r(item(a)); }),
    });
    const { container } = renderAll();
    expect(await screen.findByText("全体の傾向だけ。")).toBeInTheDocument();
    expect(container.querySelector("ul")).toBeNull();
    // 記事が1件も読み込めていない間は「根拠記事」の行を出さない。
    expect(screen.queryByText("根拠記事")).not.toBeInTheDocument();

    // get_item は非同期の IPC 越しに呼ばれる。呼ばれる前に解決しようとしないよう待つ。
    await waitFor(() => expect(resolveItem).not.toBeNull());
    await act(async () => { resolveItem!(); });
    expect(await screen.findByRole("button", { name: "記事11" })).toBeInTheDocument();
    expect(screen.getByText("根拠記事")).toBeInTheDocument();
  });
  it("shows the headline, numbered points and image cards, and leaves carded articles out of the links", async () => {
    const withImg = (a: { id: number }) => ({ ...item(a), image_url: a.id % 2 === 1 ? `https://i/${a.id}.jpg` : null });
    const ipc = setupIpc({
      get_digests: () => [{ scope: { category: "github" }, label: "GitHub", item_count: 6,
        digest: digest({ body: "見出し: 今日の要点\n- 一つ目。\n- 二つ目。", item_ids: [1, 2, 3, 4, 5, 6] }) }],
      get_item: withImg, open_item: () => undefined,
    });
    const { container } = renderAll();
    expect(await screen.findByText("今日の要点")).toHaveClass("digest-headline");
    expect(container.querySelector("ol.digest-points")?.querySelectorAll("li")).toHaveLength(2);
    expect(container.querySelector("ul.digest-bullets")).toBeNull();
    await waitFor(() => expect(container.querySelectorAll(".image-card")).toHaveLength(3));
    expect(container.querySelectorAll(".digest-refs .link-btn")).toHaveLength(3);
    expect(container.querySelector(".digest-refs")).toHaveTextContent("記事2");
    expect(container.querySelector(".digest-refs")).not.toHaveTextContent("記事1");
    await userEvent.click(container.querySelectorAll<HTMLElement>(".image-card")[0]);
    expect(ipc.last("open_item")).toEqual({ id: 1 });
  });

  it("falls back to a text link when a card image fails to load", async () => {
    const withImg = (a: { id: number }) => ({ ...item(a), image_url: `https://i/${a.id}.jpg` });
    setupIpc({
      get_digests: () => [{ scope: { category: "github" }, label: "GitHub", item_count: 3,
        digest: digest({ body: "- 一つ目。", item_ids: [1, 2, 3] }) }],
      get_item: withImg,
    });
    const { container } = renderAll();
    await waitFor(() => expect(container.querySelectorAll(".image-card")).toHaveLength(3));
    expect(container.querySelector(".digest-refs")).toBeNull();
    fireEvent.error(container.querySelectorAll(".image-card img")[0]);
    expect(container.querySelectorAll(".image-card")).toHaveLength(2);
    const refs = within(container.querySelector<HTMLElement>(".digest-refs")!);
    expect(refs.getByRole("button", { name: "記事1" })).toBeInTheDocument();
    expect(refs.queryByText(/他/)).not.toBeInTheDocument();
  });

  it("still shows the remaining count when every fetched article is carded", async () => {
    const withImg = (a: { id: number }) => a.id <= 3 ? { ...item(a), image_url: `https://i/${a.id}.jpg` } : null;
    setupIpc({
      get_digests: () => [{ scope: { category: "github" }, label: "GitHub", item_count: 5,
        digest: digest({ body: "- 一つ目。", item_ids: [1, 2, 3, 4, 5] }) }],
      get_item: withImg,
    });
    const { container } = renderAll();
    await waitFor(() => expect(container.querySelectorAll(".image-card")).toHaveLength(3));
    const refs = container.querySelector(".digest-refs");
    expect(refs).toHaveTextContent("根拠記事");
    expect(refs).toHaveTextContent("他 2件");
    expect(refs!.querySelectorAll(".link-btn")).toHaveLength(0);
  });

  it("strips bold from a headline and keeps a decorated headline line out of the body", async () => {
    setupIpc({
      get_digests: () => [{ scope: { category: "github" }, label: "GitHub", item_count: 0,
        digest: digest({ body: "**見出し:** 今日の**要点**\n- 一つ目。", item_ids: [] }) }],
      get_item: item,
    });
    const { container } = renderAll();
    expect(await screen.findByText("今日の要点")).toHaveClass("digest-headline");
    expect(container.querySelector(".digest-points")).toHaveTextContent("一つ目。");
    expect(container.querySelector(".digest-section")).not.toHaveTextContent("見出し");
  });

  it("shows no headline element for a digest without one", async () => {
    setupIpc({ get_digests: () => views, get_item: item });
    const { container } = renderAll();
    expect(await screen.findByText("一つ目。")).toBeInTheDocument();
    expect(container.querySelector(".digest-headline")).toBeNull();
  });

  it("refetches on items-changed so a section picks up the new article count", async () => {
    let itemCount = 0;
    const ipc = setupIpc({
      get_digests: () => [{ scope: { category: "huggingface" }, label: "Hugging Face", digest: null, item_count: itemCount }],
    });
    renderAll();
    expect(await screen.findByText("この期間に記事がありません。")).toBeInTheDocument();

    itemCount = 3;
    await emitEvent(EVENTS.items);
    expect(await screen.findByText("まだ要約がありません (記事 3件)")).toBeInTheDocument();
    expect(screen.queryByText("この期間に記事がありません。")).not.toBeInTheDocument();
    expect(ipc.count("get_digests")).toBe(2);
  });

  it("keeps the requested sections on items-changed and releases them on digests-changed", async () => {
    let resolveSlow: (v: DV[]) => void = () => {};
    let slow = false;
    const ipc = setupIpc({
      get_digests: () => (slow ? new Promise<DV[]>((r) => { resolveSlow = r; }) : fresh(views)),
      get_item: item, regenerate_digest: () => undefined,
    });
    renderAll();
    expect(await screen.findByText("一つ目。")).toBeInTheDocument();
    const button = screen.getByRole("button", { name: "再生成" });
    await userEvent.click(button);
    expect(button).toBeDisabled();

    await emitEvent(EVENTS.items);
    await waitFor(() => expect(ipc.count("get_digests")).toBe(2));
    await settle();
    expect(button).toBeDisabled();
    expect(section("GitHub").getByText("再生成中…")).toBeInTheDocument();

    // 完了通知による再取得が items-changed の再取得に追い越されても、依頼中の印は外れる
    slow = true;
    await emitEvent(EVENTS.digests);
    await waitFor(() => expect(ipc.count("get_digests")).toBe(3));
    slow = false;
    await emitEvent(EVENTS.items);
    await waitFor(() => expect(button).toBeEnabled());
    await act(async () => { resolveSlow(fresh(views)); });
    await settle();
    expect(button).toBeEnabled();
    expect(screen.queryByText("再生成中…")).not.toBeInTheDocument();
  });

  it("has a chat toggle that reflects and flips the panel state", async () => {
    setupIpc({ get_digests: () => fresh(views), get_item: item });
    const onToggleChat = vi.fn();
    renderAll(status, { onToggleChat });
    await screen.findByText("一つ目。");
    const open = screen.getByRole("button", { name: "チャットを開く" });
    expect(open).toHaveAttribute("aria-pressed", "false");
    await userEvent.click(open);
    expect(onToggleChat).toHaveBeenCalledTimes(1);
  });

  it("shows the toggle as pressed while the chat is open", async () => {
    setupIpc({ get_digests: () => fresh(views), get_item: item });
    renderAll(status, { chatOpen: true });
    await screen.findByText("一つ目。");
    expect(screen.getByRole("button", { name: "チャットを閉じる" })).toHaveAttribute("aria-pressed", "true");
  });

  it("asks about a finished section in chat, only for sections with a summary", async () => {
    setupIpc({ get_digests: () => fresh(views), get_item: item });
    const onAskChat = vi.fn();
    renderAll(status, { onAskChat });
    await screen.findByText("一つ目。");
    expect(screen.getAllByRole("button", { name: /についてチャットで質問/ })).toHaveLength(1);
    await userEvent.click(screen.getByRole("button", { name: "GitHubの要約についてチャットで質問" }));
    expect(onAskChat).toHaveBeenCalledWith({ type: "digest", period: "day", date: "2026-10-06", scope: "category:github", label: "GitHub" });
  });

  it("uses a topic scope for a topic section", async () => {
    setupIpc({ get_digests: () => [{ scope: { topic: 2 }, label: "介護", digest: digest({ scope: { topic: 2 } }), item_count: 3 }], get_item: item });
    const onAskChat = vi.fn();
    render(<DigestView selection={{ kind: "topic", topicId: 2 }} topics={[{ id: 2, name: "介護", keywords: [] }]}
      status={status} view="digest" onSwitchView={() => {}} {...CHAT} onAskChat={onAskChat} />);
    await userEvent.click(await screen.findByRole("button", { name: "介護の要約についてチャットで質問" }));
    expect(onAskChat).toHaveBeenCalledWith({ type: "digest", period: "day", date: "2026-10-06", scope: "topic:2", label: "介護" });
  });

  it("reports the period on mount and when it changes", async () => {
    setupIpc({ get_digests: () => fresh(views), get_item: item });
    const onPeriodChange = vi.fn();
    renderAll(status, { onPeriodChange });
    await screen.findByText("一つ目。");
    expect(onPeriodChange).toHaveBeenCalledTimes(1);
    expect(onPeriodChange).toHaveBeenLastCalledWith({ view: "digest", period: "day", date: "2026-10-06" });
    await userEvent.click(screen.getByRole("button", { name: "週" }));
    expect(onPeriodChange).toHaveBeenLastCalledWith({ view: "digest", period: "week", date: "2026-10-06" });
    expect(onPeriodChange).toHaveBeenCalledTimes(2);
  });
});
