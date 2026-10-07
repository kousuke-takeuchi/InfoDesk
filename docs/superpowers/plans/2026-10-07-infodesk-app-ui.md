# InfoDesk App UI (Plan 2b) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Tauri アプリの画面 (React) を、Phase 0 で確定したデザイン A-1〜A-12 (チャット A-4/A-12 を除く) のとおりに作り、Plan 2a のコマンドとイベントにつなぐ。

**Architecture:** `app/src` に React 19 + TypeScript。`api.ts` が全 `invoke` を型付きで包み、`types.ts` が Rust 側の JSON 形を写す。純粋ロジック (絞り込みの組み立て、日付と期間、要約本文の整形、状態判定) は `lib/` に置いて単体テストする。画面は `views/` (ダイジェスト、フィード、設定) と `components/` (サイドバー、部品)。スタイルは CSS 変数のトークン (ライト/ダーク、`prefers-color-scheme`) とクラス名。状態管理は React の state と Tauri イベント (`status-changed` / `items-changed` / `digests-changed` / `open-settings`) による再取得。

**Tech Stack:** React 19、TypeScript、Vite 8、`@tauri-apps/api` 2 (`core.invoke`、`event.listen`、`mocks.mockIPC`)、`lucide-react` (アイコン、MIT)、vitest 5 + jsdom + `@testing-library/react` + `@testing-library/user-event` + `@testing-library/jest-dom`。

**Spec:** `docs/superpowers/specs/2026-10-06-infodesk-design.md` 7節 (画面)、8節、9節。デザインは `design.pen` と `design/exports/1-digest-*.png`、`2-feed-*.png`、`3-settings-sources-*.png`、`5-settings-topics.png`、`6-settings-llm.png`、`7-settings-schedule.png`、`8-rss-add-dialog.png`、`8b-rss-dialog-states.png`、`10-digest-states.png`、`11-first-launch.png`。前提: Plan 2a (`2026-10-06-infodesk-app-backend.md`) が完了し、その末尾「Plan 2b が前提にする事実」を守る。

**範囲外:** チャットパネル (A-4、A-12、spec 12節) は Plan 3。ツールバーのチャット開閉ボタンも Plan 3 で追加する。

## Global Constraints

- 対象OSはmacOSのみ。書体はシステムフォント (`-apple-system, BlinkMacSystemFont, "Hiragino Sans", sans-serif`)。
- コミットメッセージに `Co-Authored-By` などのAI帰属トレーラーを付けない。
- 依存は `npm install` / `cargo add` で追加する。版を手書きしない。
- 各タスクのコミット前に `cd app && npm test -- --run && npm run build` と `cargo test --workspace` が通ること。`npm run build` は `tsc` を含むため、型エラーはビルド失敗になる。
- `invoke` の引数は camelCase (`topicId`)。`ItemFilter` は全フィールドを送る (Task 1 で `#[serde(default)]` を付けるが、UI 側は常に全フィールドを組み立てる)。`SidebarCounts.topics` のキーは文字列。`DigestScope` は `{"category":"github"}` または `{"topic":3}`。
- CSP: `default-src 'self'; connect-src ipc: http://ipc.localhost; img-src 'self' data:; style-src 'self' 'unsafe-inline'; font-src 'self' data:`。外部の画像、フォント、スクリプトは使えない。
- デザイントークン (spec 7節): 背景 `bg-window`、`bg-sidebar`、`bg-subtle`。線 `border`。文字 `text-primary`、`text-secondary`、`text-tertiary`。強調 `accent`、`selection`、`on-accent`。状態 `success`、`danger`、`star`。部品 `seg-selected`、`toggle-off`。角丸 5 と 7。文字は本文13px、補足11〜12px、画面見出し22px、節見出し15px。ライト/ダークの値は `design.pen` の変数と同じ (Task 2 に列挙)。
- ウィンドウ基準 1200x800、最小 960x640。サイドバー幅 220。ダイジェスト本文の幅は 720 まで。
- 記事を開くのは `open_item` コマンド経由 (Rust 側が既読にして既定ブラウザで開く)。UI から URL を直接開かない。
- 日付の境界はローカル (`Date` のローカル時刻)。週は月曜始まり。これは Rust 側 (`state.tz`) と同じ前提である。

## Review Focus

1. 記事が0件のカテゴリーやトピックを選ぶ。空状態の文言が出て、エラー表示にならない。初回起動 (全体0件) は「今すぐ収集」付きの空状態 (A-11)。Task 4 でテストする。
2. `invoke` が失敗する (例: バックエンドが `AppError` を返す)。画面は例外で白くならず、エラー行を表示する。Task 2 (サイドバー) と Task 4 でテストする。
3. ダイジェストが保留 (`pending`) または失敗 (`failed`) で `last_error` がある。理由と再試行ボタンが出る。生成中 (`AppStatus.digesting`) は「生成中…」。Task 5 でテストする。
4. RSS 追加で検証に失敗する。エラー文言が出て、「追加」は押せない。成功時は一覧が更新される。Task 6 でテストする。
5. 日付の境界: 23:30 に開いた「今日」のダイジェストと、月曜 0:30 の「今週」。`since/until` と `date` がローカル日付で計算される。Task 3 でテストする。

## File Structure

```
crates/core/src/model.rs              ItemFilter に #[serde(default)]
app/package.json                      依存とテストスクリプト
app/vitest.config.ts                  vitest (jsdom)
app/tsconfig.json                     types に jest-dom
app/src/main.tsx                      テンプレート (theme.css を読み込む)
app/src/App.tsx                       シェル: サイドバー + ビュー切替 + イベント購読
app/src/types.ts                      Rust 側の JSON 形
app/src/api.ts                        invoke / listen の型付きラッパー
app/src/lib/dates.ts                  ローカル日付、期間の開始/終了、RFC3339 変換、相対時刻
app/src/lib/filters.ts                フィードの UI 状態 → ItemFilter
app/src/lib/digest.ts                 本文の整形 (箇条書き抽出)、表示状態の判定
app/src/hooks/useTauriEvent.ts        listen の購読/解除
app/src/components/Sidebar.tsx        A-1 左列
app/src/components/Segmented.tsx      区切り選択
app/src/components/Toggle.tsx         トグル
app/src/components/ErrorLine.tsx      エラー1行
app/src/views/DigestView.tsx          A-1、A-10
app/src/views/FeedView.tsx            A-2、A-11
app/src/views/SettingsView.tsx        A-3 枠 (タブ)
app/src/views/settings/SourcesTab.tsx A-3
app/src/views/settings/AddRssDialog.tsx A-8
app/src/views/settings/TopicsTab.tsx  A-5
app/src/views/settings/LlmTab.tsx     A-6
app/src/views/settings/ScheduleTab.tsx A-7
app/src/styles/theme.css              トークン (ライト/ダーク)、リセット、共通部品
app/src/styles/layout.css             シェル、サイドバー、ツールバー、ビュー
app/src/test/setup.ts                 jest-dom、mockIPC ヘルパー
app/src/test/ipc.ts                   テスト用の IPC ハンドラー登録
app/src/**/*.test.ts(x)               テスト
```

削除: `app/src/App.css`、`app/src/assets/react.svg`、`app/public/vite.svg`、`app/public/tauri.svg` (テンプレートの残り)。

---

### Task 1: テスト基盤、型、API ラッパー

**Files:**
- Modify: `crates/core/src/model.rs` (ItemFilter), `app/package.json`, `app/tsconfig.json`
- Create: `app/vitest.config.ts`, `app/src/types.ts`, `app/src/api.ts`, `app/src/test/setup.ts`, `app/src/test/ipc.ts`, `app/src/api.test.ts`
- Delete: `app/src/App.css`, `app/src/assets/react.svg`, `app/public/vite.svg`, `app/public/tauri.svg`
- Modify: `app/src/App.tsx` (テンプレート本体を最小に), `app/src/main.tsx`

**Interfaces:**
- Consumes: Plan 2a のコマンド名と引数 (`list_items {filter}`, `get_item {id}`, `set_read {id, read}`, `set_starred {id, starred}`, `mark_all_read {filter}`, `open_item {id}`, `sidebar_counts`, `list_sources`, `add_rss_source {url, name, category}`, `update_source {id, name, category, config}`, `set_source_enabled {id, enabled}`, `delete_source {id}`, `list_topics`, `add_topic {name, keywords}`, `update_topic {id, name, keywords}`, `delete_topic {id}`, `get_digests {period, date, topicId}`, `regenerate_digest {period, date, scope}`, `collect_now`, `retry_pending_digests`, `get_settings`, `set_setting {key, value}`, `test_ollama`, `test_claude`, `set_launch_at_login {enabled}`, `get_status`)
- Produces:
  - `types.ts`: `Category`, `CATEGORIES: {id: Category; label: string}[]`, `Period`, `DigestScope`, `Item`, `ItemFilter`, `ItemOrder`, `Source`, `SourceKind`, `Topic`, `Digest`, `DigestStatus`, `DigestView`, `AppStatus`, `SidebarCounts`, `ProbeResult`, `Settings = Record<string, string>`
  - `api.ts`: `api.<name>()` 関数群 (下のコード)、`onEvent(name, handler) => Promise<UnlistenFn>`、`EVENTS` 定数
  - `test/ipc.ts`: `setupIpc(handlers: Record<string, (args: any) => unknown>) => { calls: {cmd: string; args: any}[] }` (`mockIPC` を設定し、`plugin:event|listen`/`unlisten` を自動で処理し、呼び出し履歴を返す)、`afterEach` で `clearMocks()`

- [ ] **Step 1: core の `ItemFilter` に既定値を付ける**

`crates/core/src/model.rs` の `ItemFilter` の `#[derive(...)]` の下に `#[serde(default)]` を追加する。`crates/core/src/store/items.rs` のテストに追加:

```rust
    #[test]
    fn item_filter_deserializes_with_missing_fields() {
        let f: ItemFilter = serde_json::from_str(r#"{"category":"medcare"}"#).unwrap();
        assert_eq!(f.category, Some(Category::Medcare));
        assert_eq!(f.limit, 0);
        assert!(!f.unread_only);
        assert_eq!(f.order, ItemOrder::Recent);
    }
```

Run: `cargo test -p infodesk-core item_filter_deserializes`
Expected: 失敗 (`missing field`) → `#[serde(default)]` を付けて再実行 → passed。

- [ ] **Step 2: 依存とテスト設定**

```bash
cd app
npm install lucide-react
npm install -D vitest jsdom @testing-library/react @testing-library/user-event @testing-library/jest-dom
cd ..
```

`app/package.json` の `scripts` に追加: `"test": "vitest"`。

`app/vitest.config.ts`:

```ts
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
```

`app/tsconfig.json` の `compilerOptions` に `"types": ["@testing-library/jest-dom"]` を追加する。

`app/src/test/setup.ts`:

```ts
import "@testing-library/jest-dom/vitest";
import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";
import { clearMocks } from "@tauri-apps/api/mocks";

afterEach(() => {
  cleanup();
  clearMocks();
});
```

`app/src/test/ipc.ts`:

```ts
import { mockIPC } from "@tauri-apps/api/mocks";

export type IpcHandlers = Record<string, (args: any) => unknown>;

export interface IpcLog {
  calls: { cmd: string; args: any }[];
  /** 指定コマンドの最後の引数 */
  last(cmd: string): any;
  count(cmd: string): number;
}

/**
 * Tauri の IPC をモックする。未登録のコマンドは例外にして、テストが取りこぼしを検出できるようにする。
 * イベントの listen/unlisten は常に成功させる。
 */
export function setupIpc(handlers: IpcHandlers): IpcLog {
  const calls: { cmd: string; args: any }[] = [];
  mockIPC(async (cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return undefined;
    calls.push({ cmd, args });
    const handler = handlers[cmd];
    if (!handler) throw new Error(`unmocked command: ${cmd}`);
    return handler(args);
  });
  return {
    calls,
    last: (cmd) => [...calls].reverse().find((c) => c.cmd === cmd)?.args,
    count: (cmd) => calls.filter((c) => c.cmd === cmd).length,
  };
}
```

- [ ] **Step 3: 失敗するテストを書く**

`app/src/api.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { api } from "./api";
import { setupIpc } from "./test/ipc";
import type { ItemFilter } from "./types";

describe("api", () => {
  it("sends the full filter and camelCase arguments", async () => {
    const ipc = setupIpc({
      list_items: () => [],
      get_digests: () => [],
      add_rss_source: (a) => ({ id: 1, kind: "rss", name: a.name ?? "t", category: a.category, config: a.url, enabled: true, last_fetched_at: null, last_error: null }),
    });
    const filter: ItemFilter = {
      category: "medcare", topic_id: null, source_id: null, since: null, until: null,
      text: null, unread_only: true, starred_only: false, order: "recent", limit: 50, offset: 0,
    };
    await api.listItems(filter);
    expect(ipc.last("list_items")).toEqual({ filter });

    await api.getDigests("week", "2026-10-06", 3);
    expect(ipc.last("get_digests")).toEqual({ period: "week", date: "2026-10-06", topicId: 3 });

    const s = await api.addRssSource("https://e.com/feed", null, "tech_jp");
    expect(ipc.last("add_rss_source")).toEqual({ url: "https://e.com/feed", name: null, category: "tech_jp" });
    expect(s.config).toBe("https://e.com/feed");
  });

  it("rejects with the backend message", async () => {
    setupIpc({ collect_now: () => { throw { message: "collect already running" }; } });
    await expect(api.collectNow()).rejects.toMatchObject({ message: "collect already running" });
  });
});
```

Run: `cd app && npm test -- --run`
Expected: FAIL。`./api` が存在しない。

- [ ] **Step 4: 型と API を書く**

`app/src/types.ts`:

```ts
export type Category =
  | "github" | "arxiv" | "huggingface" | "hackernews" | "tech_jp" | "tech_intl" | "medcare";

export const CATEGORIES: { id: Category; label: string }[] = [
  { id: "github", label: "GitHub" },
  { id: "arxiv", label: "arXiv" },
  { id: "huggingface", label: "Hugging Face" },
  { id: "hackernews", label: "Hacker News" },
  { id: "tech_jp", label: "国内tech" },
  { id: "tech_intl", label: "海外tech" },
  { id: "medcare", label: "医療介護" },
];

export function categoryLabel(id: Category): string {
  return CATEGORIES.find((c) => c.id === id)?.label ?? id;
}

export type Period = "day" | "week" | "month";
export type ItemOrder = "recent" | "score";
export type DigestStatus = "pending" | "done" | "failed";
export type SourceKind = "github_trending" | "arxiv" | "hf_models" | "hf_papers" | "hackernews" | "rss";

export type DigestScope = { category: Category } | { topic: number };

export interface Item {
  id: number;
  source_id: number | null;
  source_name: string;
  category: Category;
  url: string;
  title: string;
  excerpt: string;
  author: string | null;
  published_at: string | null;
  fetched_at: string;
  score: number | null;
  ai_summary: string | null;
  read: boolean;
  starred: boolean;
}

export interface ItemFilter {
  category: Category | null;
  topic_id: number | null;
  source_id: number | null;
  since: string | null;
  until: string | null;
  text: string | null;
  unread_only: boolean;
  starred_only: boolean;
  order: ItemOrder;
  limit: number;
  offset: number;
}

export interface Source {
  id: number;
  kind: SourceKind;
  name: string;
  category: Category;
  config: string;
  enabled: boolean;
  last_fetched_at: string | null;
  last_error: string | null;
}

export interface Topic {
  id: number;
  name: string;
  keywords: string[];
}

export interface Digest {
  id: number;
  period: Period;
  period_start: string;
  scope: DigestScope;
  body: string;
  item_ids: number[];
  provider: string;
  status: DigestStatus;
  attempts: number;
  created_at: string;
  last_error: string | null;
}

export interface DigestView {
  scope: DigestScope;
  label: string;
  digest: Digest | null;
}

export interface AppStatus {
  collecting: boolean;
  summarizing: boolean;
  digesting: boolean;
  last_collect_at: string | null;
  next_collect_at: string | null;
  last_error: string | null;
}

export interface SidebarCounts {
  total_unread: number;
  categories: Record<string, number>;
  topics: Record<string, number>;
}

export interface ProbeResult {
  ok: boolean;
  message: string;
  millis: number;
}

export type Settings = Record<string, string>;

/** バックエンドの AppError。invoke の reject 値。 */
export interface AppError {
  message: string;
}

export function errorMessage(e: unknown): string {
  if (e && typeof e === "object" && "message" in e && typeof (e as AppError).message === "string") {
    return (e as AppError).message;
  }
  return String(e);
}
```

`app/src/api.ts`:

```ts
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppStatus, Category, Digest, DigestScope, DigestView, Item, ItemFilter, Period,
  ProbeResult, Settings, SidebarCounts, Source, Topic,
} from "./types";

export const EVENTS = {
  status: "status-changed",
  items: "items-changed",
  digests: "digests-changed",
  openSettings: "open-settings",
} as const;

export const api = {
  getStatus: () => invoke<AppStatus>("get_status"),
  listItems: (filter: ItemFilter) => invoke<Item[]>("list_items", { filter }),
  getItem: (id: number) => invoke<Item | null>("get_item", { id }),
  setRead: (id: number, read: boolean) => invoke<void>("set_read", { id, read }),
  setStarred: (id: number, starred: boolean) => invoke<void>("set_starred", { id, starred }),
  markAllRead: (filter: ItemFilter) => invoke<number>("mark_all_read", { filter }),
  openItem: (id: number) => invoke<void>("open_item", { id }),
  sidebarCounts: () => invoke<SidebarCounts>("sidebar_counts"),
  listSources: () => invoke<Source[]>("list_sources"),
  addRssSource: (url: string, name: string | null, category: Category) =>
    invoke<Source>("add_rss_source", { url, name, category }),
  updateSource: (id: number, name: string, category: Category, config: string) =>
    invoke<void>("update_source", { id, name, category, config }),
  setSourceEnabled: (id: number, enabled: boolean) => invoke<void>("set_source_enabled", { id, enabled }),
  deleteSource: (id: number) => invoke<void>("delete_source", { id }),
  listTopics: () => invoke<Topic[]>("list_topics"),
  addTopic: (name: string, keywords: string[]) => invoke<Topic>("add_topic", { name, keywords }),
  updateTopic: (id: number, name: string, keywords: string[]) => invoke<void>("update_topic", { id, name, keywords }),
  deleteTopic: (id: number) => invoke<void>("delete_topic", { id }),
  getDigests: (period: Period, date: string, topicId: number | null) =>
    invoke<DigestView[]>("get_digests", { period, date, topicId }),
  regenerateDigest: (period: Period, date: string, scope: DigestScope) =>
    invoke<void>("regenerate_digest", { period, date, scope }),
  collectNow: () => invoke<void>("collect_now"),
  retryPendingDigests: () => invoke<void>("retry_pending_digests"),
  getSettings: () => invoke<Settings>("get_settings"),
  setSetting: (key: string, value: string) => invoke<void>("set_setting", { key, value }),
  testOllama: () => invoke<ProbeResult>("test_ollama"),
  testClaude: () => invoke<ProbeResult>("test_claude"),
  setLaunchAtLogin: (enabled: boolean) => invoke<void>("set_launch_at_login", { enabled }),
};

export type Api = typeof api;
export type { Digest };

export function onEvent(name: string, handler: () => void): Promise<UnlistenFn> {
  return listen(name, () => handler());
}
```

`app/src/App.tsx` を最小にする (Task 2 で置き換える):

```tsx
export default function App() {
  return <div className="app" />;
}
```

`app/src/main.tsx` の `import App from "./App";` の次に `import "./styles/theme.css";` と `import "./styles/layout.css";` を追加し、両ファイルを空で作る (Task 2 で中身を書く)。テンプレートの `App.css`、`assets/react.svg`、`public/vite.svg`、`public/tauri.svg` を削除する。`index.html` に `vite.svg` への参照があれば削除する。

- [ ] **Step 5: テストとビルドを確認する**

Run: `cd app && npm test -- --run && npm run build && cd .. && cargo test --workspace 2>&1 | grep -E "^test result" | grep -v "0 passed"`
Expected: vitest 2 passed、`tsc` と `vite build` が成功、core 83 / app 49 passed。

- [ ] **Step 6: コミット**

```bash
git add crates/core app
git commit -m "feat(ui): add typed Tauri API wrapper and test harness"
```

---

### Task 2: テーマ、シェル、サイドバー

**Files:**
- Create: `app/src/styles/theme.css` (中身), `app/src/styles/layout.css` (中身), `app/src/components/Sidebar.tsx`, `app/src/components/Sidebar.test.tsx`, `app/src/components/ErrorLine.tsx`, `app/src/hooks/useTauriEvent.ts`
- Modify: `app/src/App.tsx`

**Interfaces:**
- Consumes: `api.sidebarCounts`, `api.listTopics`, `api.getStatus`, `api.collectNow`, `onEvent`, `EVENTS`
- Produces:
  - `type Selection = { kind: "all" } | { kind: "category"; category: Category } | { kind: "topic"; topicId: number }` (`components/Sidebar.tsx` から export)
  - `type View = "digest" | "feed" | "settings"` (`App.tsx` から export)
  - `<Sidebar selection view onSelect onOpenSettings counts topics status />`
  - `useTauriEvent(name: string, handler: () => void)` (マウント時に `listen`、アンマウントで解除。handler は最新のものを呼ぶ)
  - `<ErrorLine message={string | null} />` (null なら何も描かない)
  - `App` は `status`、`counts`、`topics` を保持し、`status-changed` / `items-changed` / `digests-changed` で再取得。`open-settings` で設定ビューへ。ビュー本体は Task 4〜6 で差し替える (この時点ではプレースホルダー)。

- [ ] **Step 1: 失敗するテストを書く**

`app/src/components/Sidebar.test.tsx`:

```tsx
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
```

Run: `cd app && npm test -- --run`
Expected: FAIL。`./Sidebar` が存在しない。

- [ ] **Step 2: スタイルと部品を書く**

`app/src/styles/theme.css`:

```css
:root {
  color-scheme: light dark;
  --bg-window: #ffffff;
  --bg-sidebar: #f2f2f4;
  --bg-subtle: #f5f5f7;
  --border: #e0e0e3;
  --text-primary: #1d1d1f;
  --text-secondary: #6e6e73;
  --text-tertiary: #8e8e93;
  --accent: #0a6cff;
  --selection: #0a6cff1f;
  --on-accent: #ffffff;
  --success: #1e9e48;
  --danger: #d93025;
  --star: #f5a623;
  --seg-selected: #ffffff;
  --toggle-off: #d1d1d6;
  --radius-sm: 5px;
  --radius-md: 7px;
  --font: -apple-system, BlinkMacSystemFont, "Hiragino Sans", "Hiragino Kaku Gothic ProN", sans-serif;
}

@media (prefers-color-scheme: dark) {
  :root {
    --bg-window: #1e1e1e;
    --bg-sidebar: #2a2a2c;
    --bg-subtle: #2c2c2e;
    --border: #3a3a3c;
    --text-primary: #f5f5f7;
    --text-secondary: #a1a1a6;
    --text-tertiary: #7c7c80;
    --accent: #3b8cff;
    --selection: #3b8cff33;
    --success: #32d074;
    --danger: #ff6961;
    --star: #ffd60a;
    --seg-selected: #636366;
    --toggle-off: #48484a;
  }
}

*, *::before, *::after { box-sizing: border-box; }
html, body, #root { height: 100%; margin: 0; }
body {
  font-family: var(--font);
  font-size: 13px;
  color: var(--text-primary);
  background: var(--bg-window);
  -webkit-font-smoothing: antialiased;
  user-select: none;
}
button { font: inherit; color: inherit; background: none; border: 0; padding: 0; cursor: pointer; }
input, select, textarea { font: inherit; color: inherit; }
a { color: var(--accent); text-decoration: none; }

.text-secondary { color: var(--text-secondary); }
.text-tertiary { color: var(--text-tertiary); }
.small { font-size: 12px; }
.tiny { font-size: 11px; }

.btn {
  display: inline-flex; align-items: center; gap: 6px;
  padding: 6px 12px; border-radius: var(--radius-md);
  border: 1px solid var(--border); background: var(--bg-window);
  font-size: 12px; font-weight: 600;
}
.btn:disabled { opacity: .5; cursor: default; }
.btn-primary { background: var(--accent); color: var(--on-accent); border-color: transparent; }
.btn-ghost { border-color: transparent; color: var(--text-secondary); font-weight: 400; }
.btn-ghost:hover { background: var(--bg-subtle); }
.link-btn { color: var(--accent); font-size: 12px; }

.field {
  padding: 5px 10px; border-radius: var(--radius-md); border: 1px solid var(--border);
  background: var(--bg-window); font-size: 12px; min-width: 0;
}
.field:focus { outline: 2px solid var(--selection); border-color: var(--accent); }
.field-error { border-color: var(--danger); }

.segmented { display: inline-flex; gap: 2px; padding: 2px; border-radius: var(--radius-md); background: var(--bg-subtle); border: 1px solid var(--border); }
.segmented button { padding: 4px 14px; border-radius: var(--radius-sm); font-size: 12px; border: 1px solid transparent; }
.segmented button[aria-pressed="true"] { background: var(--seg-selected); border-color: var(--border); font-weight: 600; }

.toggle { position: relative; width: 32px; height: 18px; border-radius: 9px; background: var(--toggle-off); transition: background .15s; }
.toggle[aria-checked="true"] { background: var(--accent); }
.toggle::after { content: ""; position: absolute; top: 2px; left: 2px; width: 14px; height: 14px; border-radius: 50%; background: #fff; transition: transform .15s; }
.toggle[aria-checked="true"]::after { transform: translateX(14px); }

.dot { display: inline-block; width: 8px; height: 8px; border-radius: 50%; }
.dot-success { background: var(--success); }
.dot-danger { background: var(--danger); }
.dot-muted { background: var(--text-tertiary); }
.dot-accent { background: var(--accent); }

.error-line { display: flex; gap: 8px; align-items: center; color: var(--danger); font-size: 12px; }

.state-box { display: flex; gap: 10px; align-items: center; padding: 12px 14px; border-radius: var(--radius-md); background: var(--bg-subtle); color: var(--text-secondary); font-size: 12px; line-height: 1.5; }
.state-box .grow { flex: 1; }

.empty { display: flex; flex-direction: column; align-items: center; gap: 12px; max-width: 420px; margin: 0 auto; text-align: center; }
.empty h2 { font-size: 17px; margin: 0; }

.chip { display: inline-flex; align-items: center; padding: 2px 8px; border-radius: 10px; background: var(--bg-subtle); font-size: 11px; }
```

`app/src/styles/layout.css`:

```css
.app { display: flex; height: 100%; }

.sidebar {
  width: 220px; flex: none; display: flex; flex-direction: column; gap: 2px;
  padding: 16px 10px 12px; background: var(--bg-sidebar); border-right: 1px solid var(--border);
}
.sidebar-head { padding: 14px 8px 4px; font-size: 11px; font-weight: 600; color: var(--text-tertiary); }
.nav-item {
  display: flex; align-items: center; gap: 8px; width: 100%; padding: 6px 8px;
  border-radius: 6px; text-align: left; color: var(--text-primary);
}
.nav-item:hover { background: var(--bg-subtle); }
.nav-item[aria-current="true"] { background: var(--selection); font-weight: 600; }
.nav-item[aria-current="true"] svg { color: var(--accent); }
.nav-item svg { color: var(--text-secondary); flex: none; }
.nav-item .label { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.nav-item .count { font-size: 12px; color: var(--text-tertiary); }
.sidebar-spacer { flex: 1; }
.sidebar-status { display: flex; align-items: center; gap: 8px; padding: 6px 8px; font-size: 12px; color: var(--text-secondary); }
.sidebar-status svg { flex: none; }
.sidebar-status .spin { animation: spin 1s linear infinite; }
@keyframes spin { to { transform: rotate(360deg); } }
.sidebar-error { padding: 4px 8px; font-size: 11px; color: var(--danger); line-height: 1.4; }

.main { flex: 1; min-width: 0; display: flex; flex-direction: column; }
.toolbar { display: flex; align-items: center; gap: 10px; padding: 12px 24px; border-bottom: 1px solid var(--border); }
.toolbar .spacer { flex: 1; }
.search { display: flex; align-items: center; gap: 6px; width: 200px; padding: 5px 10px; border-radius: var(--radius-md); background: var(--bg-subtle); }
.search input { flex: 1; min-width: 0; border: 0; background: none; outline: none; font-size: 12px; }
.search svg { color: var(--text-secondary); }

.view { flex: 1; overflow: auto; padding: 24px 48px; }
.view-header { display: flex; align-items: flex-end; gap: 12px; margin-bottom: 24px; }
.view-header h1 { font-size: 22px; margin: 0; }
.view-header .spacer { flex: 1; }

/* ダイジェスト */
.digest-section { max-width: 720px; display: flex; flex-direction: column; gap: 10px; margin-bottom: 32px; }
.digest-head { display: flex; align-items: center; gap: 10px; }
.digest-head h2 { font-size: 15px; margin: 0; }
.digest-bullets { margin: 0; padding: 0; list-style: none; display: flex; flex-direction: column; gap: 10px; }
.digest-bullets li { display: flex; gap: 8px; line-height: 1.6; }
.digest-bullets li::before { content: "•"; color: var(--text-tertiary); }
.digest-refs { display: flex; flex-wrap: wrap; gap: 16px; align-items: center; padding-left: 16px; font-size: 12px; }

/* フィード */
.feed { padding: 20px 32px 0; }
.item-row { display: flex; gap: 12px; padding: 12px 0; border-bottom: 1px solid var(--border); }
.item-row .unread { width: 8px; padding-top: 7px; flex: none; }
.item-row .unread i { display: block; width: 8px; height: 8px; border-radius: 50%; background: var(--accent); }
.item-row .content { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 4px; }
.item-row .title { font-size: 14px; font-weight: 600; text-align: left; line-height: 1.4; color: var(--text-primary); }
.item-row.read .title { font-weight: 400; color: var(--text-secondary); }
.item-row .meta { display: flex; gap: 8px; font-size: 11px; color: var(--text-secondary); }
.item-row .meta .time, .item-row .meta .sep { color: var(--text-tertiary); }
.item-row .summary { font-size: 12px; color: var(--text-secondary); line-height: 1.5; }
.item-row .star { color: var(--text-tertiary); align-self: flex-start; }
.item-row .star[aria-pressed="true"] { color: var(--star); }

/* 設定 */
.settings { padding: 16px 32px; display: flex; flex-direction: column; gap: 32px; }
.section { display: flex; flex-direction: column; gap: 12px; }
.section h2 { font-size: 15px; margin: 0; }
.section .desc { font-size: 12px; color: var(--text-secondary); }
.setting-row { display: flex; align-items: center; gap: 16px; padding: 8px 0; border-bottom: 1px solid var(--border); }
.setting-row .label { flex: 1; display: flex; flex-direction: column; gap: 2px; }
.setting-row .label .hint { font-size: 11px; color: var(--text-tertiary); }
.table-head, .table-row { display: grid; gap: 12px; align-items: center; padding: 10px 0; border-bottom: 1px solid var(--border); }
.table-head { font-size: 11px; font-weight: 600; color: var(--text-tertiary); padding: 8px 0; }
.sources-grid { grid-template-columns: 44px 1fr 110px 90px 230px 56px; }
.topics-grid { grid-template-columns: 200px 1fr 60px 56px; }
.cell-name { display: flex; flex-direction: column; gap: 2px; }
.cell-name .url { font-size: 11px; color: var(--text-tertiary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cell-status { display: flex; align-items: center; gap: 6px; font-size: 12px; color: var(--text-secondary); }
.cell-status.error { color: var(--danger); }
.cell-actions { display: flex; gap: 6px; color: var(--text-secondary); }
.card { display: flex; flex-direction: column; gap: 12px; padding: 16px; border-radius: 10px; background: var(--bg-subtle); max-width: 560px; }
.card .row { display: flex; align-items: center; gap: 12px; }
.card .row > label { width: 90px; font-size: 12px; }
.card .row .field { flex: 1; }
.card .actions { display: flex; justify-content: flex-end; gap: 8px; }

/* ダイアログ */
.overlay { position: fixed; inset: 0; background: #00000040; display: flex; align-items: center; justify-content: center; }
.dialog { width: 480px; display: flex; flex-direction: column; gap: 14px; padding: 20px; border-radius: 12px; background: var(--bg-sidebar); border: 1px solid var(--border); box-shadow: 0 12px 40px #00000040; }
.dialog h2 { font-size: 15px; margin: 0; }
.dialog .row { display: flex; flex-direction: column; gap: 6px; }
.dialog .row label { font-size: 12px; color: var(--text-secondary); }
.dialog .validation { display: flex; gap: 8px; align-items: center; font-size: 12px; color: var(--text-secondary); min-height: 20px; }
.dialog .validation.error { color: var(--danger); }
.dialog .actions { display: flex; justify-content: flex-end; gap: 8px; padding-top: 6px; }
```

`app/src/components/ErrorLine.tsx`:

```tsx
import { CircleAlert } from "lucide-react";

export function ErrorLine({ message }: { message: string | null }) {
  if (!message) return null;
  return (
    <div className="error-line" role="alert">
      <CircleAlert size={14} />
      <span>{message}</span>
    </div>
  );
}
```

`app/src/hooks/useTauriEvent.ts`:

```ts
import { useEffect, useRef } from "react";
import { onEvent } from "../api";

/** Tauri のイベントを購読する。handler は常に最新のものが呼ばれる。 */
export function useTauriEvent(name: string, handler: () => void) {
  const latest = useRef(handler);
  latest.current = handler;
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    onEvent(name, () => latest.current()).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [name]);
}
```

`app/src/components/Sidebar.tsx`:

```tsx
import {
  Box, FileText, Flame, Github, Globe, Hash, HeartPulse, Layers, Newspaper, RefreshCw, Settings,
} from "lucide-react";
import type { ComponentType } from "react";
import { CATEGORIES, type AppStatus, type Category, type SidebarCounts, type Topic } from "../types";
import { formatClock } from "../lib/dates";

export type Selection =
  | { kind: "all" }
  | { kind: "category"; category: Category }
  | { kind: "topic"; topicId: number };

export type View = "digest" | "feed" | "settings";

const ICONS: Record<Category, ComponentType<{ size?: number }>> = {
  github: Github, arxiv: FileText, huggingface: Box, hackernews: Flame,
  tech_jp: Newspaper, tech_intl: Globe, medcare: HeartPulse,
};

interface Props {
  selection: Selection;
  view: View;
  counts: SidebarCounts;
  topics: Topic[];
  status: AppStatus;
  onSelect: (s: Selection) => void;
  onOpenSettings: () => void;
  onCollectNow: () => void;
}

function sameSelection(a: Selection, b: Selection): boolean {
  if (a.kind !== b.kind) return false;
  if (a.kind === "category" && b.kind === "category") return a.category === b.category;
  if (a.kind === "topic" && b.kind === "topic") return a.topicId === b.topicId;
  return true;
}

function NavItem({ icon: Icon, label, count, current, onClick }: {
  icon: ComponentType<{ size?: number }>; label: string; count?: number; current: boolean; onClick: () => void;
}) {
  return (
    <button className="nav-item" aria-current={current ? "true" : undefined} onClick={onClick}>
      <Icon size={16} />
      <span className="label">{label}</span>
      {count ? <span className="count">{count}</span> : null}
    </button>
  );
}

export function Sidebar({ selection, view, counts, topics, status, onSelect, onOpenSettings, onCollectNow }: Props) {
  const inContent = view !== "settings";
  const current = (s: Selection) => inContent && sameSelection(selection, s);
  return (
    <nav className="sidebar" aria-label="ナビゲーション">
      <NavItem icon={Layers} label="すべて" count={counts.total_unread}
        current={current({ kind: "all" })} onClick={() => onSelect({ kind: "all" })} />
      <div className="sidebar-head">カテゴリー</div>
      {CATEGORIES.map((c) => (
        <NavItem key={c.id} icon={ICONS[c.id]} label={c.label} count={counts.categories[c.id] ?? 0}
          current={current({ kind: "category", category: c.id })}
          onClick={() => onSelect({ kind: "category", category: c.id })} />
      ))}
      {topics.length > 0 && <div className="sidebar-head">トピック</div>}
      {topics.map((t) => (
        <NavItem key={t.id} icon={Hash} label={t.name} count={counts.topics[String(t.id)] ?? 0}
          current={current({ kind: "topic", topicId: t.id })}
          onClick={() => onSelect({ kind: "topic", topicId: t.id })} />
      ))}
      <div className="sidebar-spacer" />
      <button className="sidebar-status" onClick={onCollectNow} disabled={status.collecting} title="今すぐ収集">
        <RefreshCw size={14} className={status.collecting ? "spin" : undefined} />
        <span>
          {status.collecting ? "収集中…" : status.last_collect_at ? `最終収集 ${formatClock(status.last_collect_at)}` : "未収集"}
        </span>
      </button>
      {status.last_error && <div className="sidebar-error">{status.last_error}</div>}
      <NavItem icon={Settings} label="設定" current={view === "settings"} onClick={onOpenSettings} />
    </nav>
  );
}
```

`formatClock` は Task 3 の `lib/dates.ts` にある。このタスクでは先に最小版を作る。`app/src/lib/dates.ts`:

```ts
/** "09:00" 形式のローカル時刻。 */
export function formatClock(iso: string): string {
  const d = new Date(iso);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}
```

`app/src/App.tsx`:

```tsx
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
```

- [ ] **Step 3: テストとビルドを確認する**

Run: `cd app && npm test -- --run && npm run build`
Expected: Sidebar 3 件を含めて全件 passed、ビルド成功。

- [ ] **Step 4: コミット**

```bash
git add app
git commit -m "feat(ui): add theme tokens, app shell, and sidebar"
```

---

### Task 3: 日付、絞り込み、要約本文の純粋ロジック

**Files:**
- Modify: `app/src/lib/dates.ts`
- Create: `app/src/lib/dates.test.ts`, `app/src/lib/filters.ts`, `app/src/lib/filters.test.ts`, `app/src/lib/digest.ts`, `app/src/lib/digest.test.ts`

**Interfaces:**
- Produces (`lib/dates.ts`):
  - `localDateString(d: Date) -> "YYYY-MM-DD"` (ローカル日付)
  - `startOfLocalDay(d: Date) -> Date`、`addDays(d, n) -> Date`
  - `periodStart(period, d: Date) -> Date` (日: その日、週: 月曜、月: 1日。全てローカル 0:00)
  - `periodEnd(period, start: Date) -> Date` (排他的)
  - `periodHeading(period, d: Date) -> { title: string; range: string }` (例: 今日 → `{ title: "今日のダイジェスト", range: "2026年10月6日(火)" }`、週 → `"今週のダイジェスト"`, `"2026年10月5日(月) 〜 10月11日(日)"`、月 → `"今月のダイジェスト"`, `"2026年10月"`)
  - `relativeTime(iso: string, now: Date) -> string` (`たった今`、`N分前`、`N時間前`、`昨日`、`N日前`、それ以上は `M月D日`)
  - `formatClock(iso)` (Task 2 で作成済み)
- Produces (`lib/filters.ts`):
  - `type FeedRange = "today" | "3d" | "week" | "month" | "all"`、`FEED_RANGES: {id: FeedRange; label: string}[]`
  - `interface FeedState { selection: Selection; range: FeedRange; sourceId: number | null; unreadOnly: boolean; starredOnly: boolean; text: string }`
  - `buildItemFilter(state: FeedState, now: Date, limit = 100) -> ItemFilter` (全フィールドを埋める。`since` はローカル 0:00 の `today - N` を RFC3339 (UTC) で。`all` は null。`text` は trim、空なら null)
- Produces (`lib/digest.ts`):
  - `parseDigestBody(body: string) -> { bullets: string[]; paragraphs: string[] }` (行頭の `-`、`*`、`・`、`•`、`1.` を箇条書きとして扱い、記号を除く。空行は捨てる。残りは段落)
  - `type DigestDisplay = { kind: "done"; digest: Digest } | { kind: "generating" } | { kind: "pending"; error: string | null; attempts: number } | { kind: "failed"; error: string | null; attempts: number } | { kind: "none" }`
  - `digestDisplay(view: DigestView, status: AppStatus) -> DigestDisplay` (`digest` が done → done。`status.digesting` かつ digest が null か pending → generating。pending → pending。failed → failed。null → none)

- [ ] **Step 1: 失敗するテストを書く**

`app/src/lib/dates.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { addDays, localDateString, periodEnd, periodHeading, periodStart, relativeTime, startOfLocalDay } from "./dates";

const at = (s: string) => new Date(s);

describe("dates", () => {
  it("uses local dates at the boundaries", () => {
    // Review Focus 5: 23:30 ローカルは同じ日、翌 0:30 は次の日。
    const late = new Date(2026, 9, 6, 23, 30);
    const early = new Date(2026, 9, 7, 0, 30);
    expect(localDateString(late)).toBe("2026-10-06");
    expect(localDateString(early)).toBe("2026-10-07");
    expect(startOfLocalDay(late).getHours()).toBe(0);
    expect(localDateString(startOfLocalDay(early))).toBe("2026-10-07");
    expect(localDateString(addDays(early, -7))).toBe("2026-09-30");
  });

  it("computes period starts and ends with Monday weeks", () => {
    const mon = new Date(2026, 9, 5, 0, 30); // 月曜 0:30
    const sun = new Date(2026, 9, 11, 12, 0);
    expect(localDateString(periodStart("day", mon))).toBe("2026-10-05");
    expect(localDateString(periodStart("week", mon))).toBe("2026-10-05");
    expect(localDateString(periodStart("week", sun))).toBe("2026-10-05");
    expect(localDateString(periodStart("month", sun))).toBe("2026-10-01");
    expect(localDateString(periodEnd("week", periodStart("week", sun)))).toBe("2026-10-12");
    expect(localDateString(periodEnd("month", new Date(2026, 11, 1)))).toBe("2027-01-01");
  });

  it("formats headings", () => {
    const d = new Date(2026, 9, 6, 9, 0);
    expect(periodHeading("day", d)).toEqual({ title: "今日のダイジェスト", range: "2026年10月6日(火)" });
    expect(periodHeading("week", d)).toEqual({ title: "今週のダイジェスト", range: "2026年10月5日(月) 〜 10月11日(日)" });
    expect(periodHeading("month", d)).toEqual({ title: "今月のダイジェスト", range: "2026年10月" });
  });

  it("formats relative time", () => {
    const now = new Date(2026, 9, 6, 12, 0);
    expect(relativeTime(new Date(2026, 9, 6, 11, 59, 40).toISOString(), now)).toBe("たった今");
    expect(relativeTime(new Date(2026, 9, 6, 11, 30).toISOString(), now)).toBe("30分前");
    expect(relativeTime(new Date(2026, 9, 6, 9, 0).toISOString(), now)).toBe("3時間前");
    expect(relativeTime(new Date(2026, 9, 5, 20, 0).toISOString(), now)).toBe("昨日");
    expect(relativeTime(new Date(2026, 9, 3, 20, 0).toISOString(), now)).toBe("3日前");
    expect(relativeTime(new Date(2026, 8, 20, 20, 0).toISOString(), now)).toBe("9月20日");
    expect(relativeTime(at("2026-10-06T00:00:00Z"), now)).toMatch(/前|昨日|今/);
  });
});
```

`relativeTime` の最後の行は `string` 引数のため、`at(...)` を `at(...).toISOString()` に直して書く (型エラーを避ける)。

`app/src/lib/filters.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { buildItemFilter, type FeedState } from "./filters";

const base: FeedState = {
  selection: { kind: "all" }, range: "all", sourceId: null, unreadOnly: false, starredOnly: false, text: "",
};
const now = new Date(2026, 9, 6, 23, 30);

describe("buildItemFilter", () => {
  it("fills every field and maps the selection", () => {
    const f = buildItemFilter(base, now);
    expect(f).toEqual({
      category: null, topic_id: null, source_id: null, since: null, until: null, text: null,
      unread_only: false, starred_only: false, order: "recent", limit: 100, offset: 0,
    });
    expect(buildItemFilter({ ...base, selection: { kind: "category", category: "medcare" } }, now).category).toBe("medcare");
    expect(buildItemFilter({ ...base, selection: { kind: "topic", topicId: 4 } }, now).topic_id).toBe(4);
  });

  it("computes since from the local day start", () => {
    const today = buildItemFilter({ ...base, range: "today" }, now);
    expect(today.since).toBe(new Date(2026, 9, 6, 0, 0).toISOString());
    expect(today.until).toBeNull();
    const week = buildItemFilter({ ...base, range: "week" }, now);
    expect(week.since).toBe(new Date(2026, 9, 6 - 6, 0, 0).toISOString());
    expect(buildItemFilter({ ...base, range: "month" }, now).since).toBe(new Date(2026, 8, 7, 0, 0).toISOString());
    expect(buildItemFilter({ ...base, range: "3d" }, now).since).toBe(new Date(2026, 9, 4, 0, 0).toISOString());
  });

  it("trims text and passes flags", () => {
    const f = buildItemFilter({ ...base, text: "  認知症 ", unreadOnly: true, starredOnly: true, sourceId: 7 }, now, 30);
    expect(f.text).toBe("認知症");
    expect(f.unread_only).toBe(true);
    expect(f.starred_only).toBe(true);
    expect(f.source_id).toBe(7);
    expect(f.limit).toBe(30);
    expect(buildItemFilter({ ...base, text: "   " }, now).text).toBeNull();
  });
});
```

`app/src/lib/digest.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { digestDisplay, parseDigestBody } from "./digest";
import type { AppStatus, Digest, DigestView } from "../types";

const status: AppStatus = { collecting: false, summarizing: false, digesting: false, last_collect_at: null, next_collect_at: null, last_error: null };
const digest = (over: Partial<Digest>): Digest => ({
  id: 1, period: "day", period_start: "2026-10-06", scope: { category: "github" }, body: "", item_ids: [],
  provider: "ollama:gemma4:26b", status: "done", attempts: 0, created_at: "2026-10-06T00:00:00Z", last_error: null, ...over,
});
const view = (d: Digest | null): DigestView => ({ scope: { category: "github" }, label: "GitHub", digest: d });

describe("parseDigestBody", () => {
  it("extracts bullets and keeps the rest as paragraphs", () => {
    const body = "- 一つ目。\n* 二つ目。\n・三つ目\n• 四つ目\n1. 五つ目\n\n全体として伸びている。";
    expect(parseDigestBody(body)).toEqual({
      bullets: ["一つ目。", "二つ目。", "三つ目", "四つ目", "五つ目"],
      paragraphs: ["全体として伸びている。"],
    });
    expect(parseDigestBody("")).toEqual({ bullets: [], paragraphs: [] });
  });
});

describe("digestDisplay", () => {
  it("maps statuses", () => {
    expect(digestDisplay(view(digest({ body: "x" })), status).kind).toBe("done");
    expect(digestDisplay(view(null), status).kind).toBe("none");
    expect(digestDisplay(view(null), { ...status, digesting: true }).kind).toBe("generating");
    const pending = digestDisplay(view(digest({ status: "pending", attempts: 1, last_error: "ollama down" })), status);
    expect(pending).toEqual({ kind: "pending", error: "ollama down", attempts: 1 });
    expect(digestDisplay(view(digest({ status: "pending" })), { ...status, digesting: true }).kind).toBe("generating");
    const failed = digestDisplay(view(digest({ status: "failed", attempts: 3, last_error: "404" })), status);
    expect(failed).toEqual({ kind: "failed", error: "404", attempts: 3 });
    // 生成済みは digesting 中でもそのまま表示する。
    expect(digestDisplay(view(digest({ body: "x" })), { ...status, digesting: true }).kind).toBe("done");
  });
});
```

Run: `cd app && npm test -- --run`
Expected: FAIL (3 ファイルが import に失敗)。

- [ ] **Step 2: 実装する**

`app/src/lib/dates.ts` を次に置き換える:

```ts
import type { Period } from "../types";

const WEEKDAYS = ["日", "月", "火", "水", "木", "金", "土"];

export function formatClock(iso: string): string {
  const d = new Date(iso);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

export function localDateString(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

export function startOfLocalDay(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate());
}

export function addDays(d: Date, n: number): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + n, d.getHours(), d.getMinutes());
}

/** 期間の開始 (ローカル 0:00)。週は月曜始まり。 */
export function periodStart(period: Period, d: Date): Date {
  const day = startOfLocalDay(d);
  if (period === "day") return day;
  if (period === "week") {
    const offset = (day.getDay() + 6) % 7; // 月曜 = 0
    return new Date(day.getFullYear(), day.getMonth(), day.getDate() - offset);
  }
  return new Date(day.getFullYear(), day.getMonth(), 1);
}

/** 期間の終了 (排他的)。`start` は periodStart の戻り値。 */
export function periodEnd(period: Period, start: Date): Date {
  if (period === "day") return new Date(start.getFullYear(), start.getMonth(), start.getDate() + 1);
  if (period === "week") return new Date(start.getFullYear(), start.getMonth(), start.getDate() + 7);
  return new Date(start.getFullYear(), start.getMonth() + 1, 1);
}

function dateJa(d: Date, withYear: boolean): string {
  const y = withYear ? `${d.getFullYear()}年` : "";
  return `${y}${d.getMonth() + 1}月${d.getDate()}日(${WEEKDAYS[d.getDay()]})`;
}

export function periodHeading(period: Period, d: Date): { title: string; range: string } {
  const start = periodStart(period, d);
  if (period === "day") return { title: "今日のダイジェスト", range: dateJa(start, true) };
  if (period === "week") {
    const last = addDays(periodEnd("week", start), -1);
    return { title: "今週のダイジェスト", range: `${dateJa(start, true)} 〜 ${dateJa(last, false)}` };
  }
  return { title: "今月のダイジェスト", range: `${start.getFullYear()}年${start.getMonth() + 1}月` };
}

export function relativeTime(iso: string, now: Date): string {
  const t = new Date(iso);
  const diffMin = Math.floor((now.getTime() - t.getTime()) / 60000);
  if (diffMin < 1) return "たった今";
  if (diffMin < 60) return `${diffMin}分前`;
  const diffHours = Math.floor(diffMin / 60);
  if (diffHours < 24 && startOfLocalDay(t).getTime() === startOfLocalDay(now).getTime()) return `${diffHours}時間前`;
  const dayDiff = Math.round((startOfLocalDay(now).getTime() - startOfLocalDay(t).getTime()) / 86400000);
  if (dayDiff <= 0) return `${Math.max(diffHours, 1)}時間前`;
  if (dayDiff === 1) return "昨日";
  if (dayDiff < 7) return `${dayDiff}日前`;
  return `${t.getMonth() + 1}月${t.getDate()}日`;
}
```

`app/src/lib/filters.ts`:

```ts
import type { Selection } from "../components/Sidebar";
import type { ItemFilter } from "../types";
import { addDays, startOfLocalDay } from "./dates";

export type FeedRange = "today" | "3d" | "week" | "month" | "all";

export const FEED_RANGES: { id: FeedRange; label: string; days: number | null }[] = [
  { id: "today", label: "今日", days: 0 },
  { id: "3d", label: "3日", days: 2 },
  { id: "week", label: "1週間", days: 6 },
  { id: "month", label: "1ヶ月", days: 29 },
  { id: "all", label: "すべて", days: null },
];

export interface FeedState {
  selection: Selection;
  range: FeedRange;
  sourceId: number | null;
  unreadOnly: boolean;
  starredOnly: boolean;
  text: string;
}

/** 画面の状態からバックエンドの ItemFilter を作る。全フィールドを埋める。 */
export function buildItemFilter(state: FeedState, now: Date, limit = 100): ItemFilter {
  const range = FEED_RANGES.find((r) => r.id === state.range);
  const since = range && range.days !== null ? addDays(startOfLocalDay(now), -range.days).toISOString() : null;
  const text = state.text.trim();
  return {
    category: state.selection.kind === "category" ? state.selection.category : null,
    topic_id: state.selection.kind === "topic" ? state.selection.topicId : null,
    source_id: state.sourceId,
    since,
    until: null,
    text: text.length > 0 ? text : null,
    unread_only: state.unreadOnly,
    starred_only: state.starredOnly,
    order: "recent",
    limit,
    offset: 0,
  };
}
```

`app/src/lib/digest.ts`:

```ts
import type { AppStatus, Digest, DigestView } from "../types";

const BULLET = /^\s*(?:[-*・•]|\d+[.)])\s+(.*)$/;

export function parseDigestBody(body: string): { bullets: string[]; paragraphs: string[] } {
  const bullets: string[] = [];
  const paragraphs: string[] = [];
  for (const raw of body.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line) continue;
    const m = BULLET.exec(line);
    if (m) bullets.push(m[1].trim());
    else paragraphs.push(line);
  }
  return { bullets, paragraphs };
}

export type DigestDisplay =
  | { kind: "done"; digest: Digest }
  | { kind: "generating" }
  | { kind: "pending"; error: string | null; attempts: number }
  | { kind: "failed"; error: string | null; attempts: number }
  | { kind: "none" };

export function digestDisplay(view: DigestView, status: AppStatus): DigestDisplay {
  const d = view.digest;
  if (d && d.status === "done") return { kind: "done", digest: d };
  if (status.digesting && (!d || d.status === "pending")) return { kind: "generating" };
  if (d && d.status === "pending") return { kind: "pending", error: d.last_error, attempts: d.attempts };
  if (d && d.status === "failed") return { kind: "failed", error: d.last_error, attempts: d.attempts };
  return { kind: "none" };
}
```

- [ ] **Step 3: テストが通ることを確認する**

Run: `cd app && npm test -- --run && npm run build`
Expected: 全件 passed、ビルド成功。

- [ ] **Step 4: コミット**

```bash
git add app
git commit -m "feat(ui): add date, filter, and digest display helpers"
```

---

### Task 4: フィード画面

**Files:**
- Create: `app/src/views/FeedView.tsx`, `app/src/views/FeedView.test.tsx`, `app/src/components/Segmented.tsx`
- Modify: `app/src/App.tsx` (ビュー切替にフィードを差し込む)

**Interfaces:**
- Consumes: `api.listItems/setRead/setStarred/openItem/markAllRead/listSources/collectNow`, `buildItemFilter`, `FEED_RANGES`, `relativeTime`, `useTauriEvent`, `EVENTS.items`
- Produces:
  - `<Segmented options={{id,label}[]} value onChange />` (`aria-pressed`)
  - `<FeedView selection topics status onOpenSettings />`。ツールバー: ダイジェスト/フィード切替 (`App` が持つ `view` の変更は `onSwitchView` で受ける)、期間、ソース、未読のみ、スター、検索。見出し: 選択名 (すべて / カテゴリー名 / トピック名) と未読件数、「すべて既読にする」。
  - `App.tsx`: `view === "feed"` で `<FeedView>`、ツールバーの切替は各ビューが描く (`<ViewSwitch view onChange />` を `components/Segmented.tsx` に同居させる)

- [ ] **Step 1: 失敗するテストを書く**

`app/src/views/FeedView.test.tsx`:

```tsx
import { describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { FeedView } from "./FeedView";
import { setupIpc } from "../test/ipc";
import type { AppStatus, Item } from "../types";

const status: AppStatus = { collecting: false, summarizing: false, digesting: false, last_collect_at: "2026-10-06T00:00:00Z", next_collect_at: null, last_error: null };
const item = (over: Partial<Item>): Item => ({
  id: 1, source_id: 1, source_name: "Google News: 認知症", category: "medcare", url: "https://e.com/1",
  title: "認知症の新薬", excerpt: "", author: null, published_at: null, fetched_at: new Date().toISOString(),
  score: null, ai_summary: "要約文。", read: false, starred: false, ...over,
});

function renderFeed(items: Item[], extra: Record<string, (a: any) => unknown> = {}, st: AppStatus = status) {
  const ipc = setupIpc({
    list_items: () => items,
    list_sources: () => [{ id: 1, kind: "rss", name: "Google News: 認知症", category: "medcare", config: "u", enabled: true, last_fetched_at: null, last_error: null }],
    set_read: () => undefined, set_starred: () => undefined, open_item: () => undefined,
    mark_all_read: () => items.filter((i) => !i.read).length, collect_now: () => undefined,
    ...extra,
  });
  render(
    <FeedView selection={{ kind: "category", category: "medcare" }} topics={[]} status={st}
      view="feed" onSwitchView={() => {}} />,
  );
  return ipc;
}

describe("FeedView", () => {
  it("renders items with unread, summary, and sends the full filter", async () => {
    const ipc = renderFeed([item({}), item({ id: 2, title: "既読の記事", read: true, starred: true, score: 42 })]);
    expect(await screen.findByText("認知症の新薬")).toBeInTheDocument();
    expect(screen.getByText("要約文。")).toBeInTheDocument();
    expect(screen.getByText("既読の記事").closest(".item-row")).toHaveClass("read");
    expect(screen.getByText("42")).toBeInTheDocument();
    const filter = ipc.last("list_items").filter;
    expect(filter.category).toBe("medcare");
    expect(filter).toMatchObject({ unread_only: false, starred_only: false, order: "recent", offset: 0 });
    expect(Object.keys(filter).sort()).toEqual(
      ["category", "limit", "offset", "order", "since", "source_id", "starred_only", "text", "topic_id", "unread_only", "until"],
    );
    expect(screen.getByRole("heading", { name: /医療介護/ })).toBeInTheDocument();
    expect(screen.getByText("未読 1件")).toBeInTheDocument();
  });

  it("opens, stars, filters, and marks all read", async () => {
    const ipc = renderFeed([item({})]);
    await screen.findByText("認知症の新薬");
    await userEvent.click(screen.getByRole("button", { name: "認知症の新薬" }));
    expect(ipc.last("open_item")).toEqual({ id: 1 });

    await userEvent.click(screen.getByRole("button", { name: "スターを付ける" }));
    expect(ipc.last("set_starred")).toEqual({ id: 1, starred: true });

    await userEvent.click(screen.getByRole("button", { name: "未読のみ" }));
    await waitFor(() => expect(ipc.last("list_items").filter.unread_only).toBe(true));

    await userEvent.type(screen.getByPlaceholderText("検索"), "新薬");
    await waitFor(() => expect(ipc.last("list_items").filter.text).toBe("新薬"));

    await userEvent.click(screen.getByRole("button", { name: "すべて既読にする" }));
    expect(ipc.last("mark_all_read").filter.category).toBe("medcare");
  });

  it("shows the first-launch empty state and the error line", async () => {
    // Review Focus 1 と 2
    const ipc = renderFeed([], {}, { ...status, last_collect_at: null });
    expect(await screen.findByText("まだ記事がありません")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "今すぐ収集" }));
    expect(ipc.count("collect_now")).toBe(1);

    setupIpc({ list_items: () => { throw { message: "database lock poisoned" }; }, list_sources: () => [] });
    render(<FeedView selection={{ kind: "all" }} topics={[]} status={status} view="feed" onSwitchView={() => {}} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("database lock poisoned");
  });

  it("shows the filtered empty state when items exist elsewhere", async () => {
    renderFeed([], { list_items: (a) => (a.filter.unread_only ? [] : [item({ read: true })]) });
    await screen.findByText("認知症の新薬");
    await userEvent.click(screen.getByRole("button", { name: "未読のみ" }));
    expect(await screen.findByText("条件に一致する記事がありません")).toBeInTheDocument();
  });
});
```

Run: `cd app && npm test -- --run`
Expected: FAIL。`./FeedView` が存在しない。

- [ ] **Step 2: 実装する**

`app/src/components/Segmented.tsx`:

```tsx
import type { View } from "./Sidebar";

export function Segmented<T extends string>({ options, value, onChange, ariaLabel }: {
  options: { id: T; label: string }[]; value: T; onChange: (v: T) => void; ariaLabel?: string;
}) {
  return (
    <div className="segmented" role="group" aria-label={ariaLabel}>
      {options.map((o) => (
        <button key={o.id} aria-pressed={o.id === value} onClick={() => onChange(o.id)}>{o.label}</button>
      ))}
    </div>
  );
}

const VIEW_OPTIONS: { id: Exclude<View, "settings">; label: string }[] = [
  { id: "digest", label: "ダイジェスト" },
  { id: "feed", label: "フィード" },
];

export function ViewSwitch({ view, onChange }: { view: View; onChange: (v: View) => void }) {
  return <Segmented options={VIEW_OPTIONS} value={view === "settings" ? "digest" : view} onChange={onChange} ariaLabel="表示" />;
}
```

`app/src/views/FeedView.tsx`:

```tsx
import { Newspaper, Search, Star } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { api, EVENTS } from "../api";
import { ErrorLine } from "../components/ErrorLine";
import { Segmented, ViewSwitch } from "../components/Segmented";
import type { Selection, View } from "../components/Sidebar";
import { useTauriEvent } from "../hooks/useTauriEvent";
import { relativeTime } from "../lib/dates";
import { buildItemFilter, FEED_RANGES, type FeedRange, type FeedState } from "../lib/filters";
import { categoryLabel, errorMessage, type AppStatus, type Item, type Source, type Topic } from "../types";

interface Props {
  selection: Selection;
  topics: Topic[];
  status: AppStatus;
  view: View;
  onSwitchView: (v: View) => void;
}

function selectionTitle(selection: Selection, topics: Topic[]): string {
  if (selection.kind === "all") return "すべての記事";
  if (selection.kind === "category") return categoryLabel(selection.category);
  return topics.find((t) => t.id === selection.topicId)?.name ?? "トピック";
}

export function FeedView({ selection, topics, status, view, onSwitchView }: Props) {
  const [range, setRange] = useState<FeedRange>("week");
  const [sourceId, setSourceId] = useState<number | null>(null);
  const [unreadOnly, setUnreadOnly] = useState(false);
  const [starredOnly, setStarredOnly] = useState(false);
  const [text, setText] = useState("");
  const [items, setItems] = useState<Item[] | null>(null);
  const [sources, setSources] = useState<Source[]>([]);
  const [error, setError] = useState<string | null>(null);

  const state: FeedState = useMemo(
    () => ({ selection, range, sourceId, unreadOnly, starredOnly, text }),
    [selection, range, sourceId, unreadOnly, starredOnly, text],
  );
  // 一度も収集していなければ初回起動の空状態、それ以外の0件は絞り込みの結果として扱う。
  const neverCollected = status.last_collect_at === null;

  const load = useCallback(() => {
    api.listItems(buildItemFilter(state, new Date()))
      .then((list) => { setItems(list); setError(null); })
      .catch((e) => setError(errorMessage(e)));
  }, [state]);

  useEffect(load, [load]);
  useEffect(() => { api.listSources().then(setSources).catch(() => setSources([])); }, []);
  useTauriEvent(EVENTS.items, load);

  const run = (p: Promise<unknown>) => p.then(load).catch((e) => setError(errorMessage(e)));
  const unread = items?.filter((i) => !i.read).length ?? 0;
  const now = new Date();

  return (
    <>
      <div className="toolbar">
        <ViewSwitch view={view} onChange={onSwitchView} />
        <div className="spacer" />
        <Segmented options={FEED_RANGES} value={range} onChange={setRange} ariaLabel="期間" />
        <select className="field" aria-label="ソース" value={sourceId ?? ""} onChange={(e) => setSourceId(e.target.value ? Number(e.target.value) : null)}>
          <option value="">すべてのソース</option>
          {sources.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
        </select>
        <button className={`btn ${unreadOnly ? "btn-primary" : ""}`} aria-pressed={unreadOnly} onClick={() => setUnreadOnly(!unreadOnly)}>未読のみ</button>
        <button className={`btn ${starredOnly ? "btn-primary" : ""}`} aria-pressed={starredOnly} onClick={() => setStarredOnly(!starredOnly)}>
          <Star size={12} /> スター
        </button>
        <label className="search">
          <Search size={14} />
          <input placeholder="検索" value={text} onChange={(e) => setText(e.target.value)} />
        </label>
      </div>
      <div className="view feed">
        <ErrorLine message={error} />
        <div className="view-header">
          <h1>{selectionTitle(selection, topics)}</h1>
          <span className="text-secondary">未読 {unread}件</span>
          <div className="spacer" />
          {items && items.length > 0 && (
            <button className="link-btn" onClick={() => run(api.markAllRead(buildItemFilter(state, new Date())))}>すべて既読にする</button>
          )}
        </div>
        {items && items.length === 0 && neverCollected && !error && (
          <div className="empty">
            <Newspaper size={40} className="text-tertiary" />
            <h2>まだ記事がありません</h2>
            <p className="text-secondary">収集を始めると、GitHub、arXiv、Hugging Face、Hacker News、国内外のtechニュース、医療介護ニュースが集まり、朝7時に日次要約が生成されます。</p>
            <div style={{ display: "flex", gap: 8 }}>
              <button className="btn btn-primary" disabled={status.collecting} onClick={() => run(api.collectNow())}>今すぐ収集</button>
            </div>
            <p className="tiny text-tertiary">収集には1〜2分かかります。進行状況はサイドバー下部に表示されます。</p>
          </div>
        )}
        {items && items.length === 0 && !neverCollected && !error && (
          <div className="state-box"><span>条件に一致する記事がありません</span></div>
        )}
        {items?.map((it) => (
          <article key={it.id} className={`item-row ${it.read ? "read" : ""}`}>
            <div className="unread">{!it.read && <i aria-label="未読" />}</div>
            <div className="content">
              <button className="title" onClick={() => run(api.openItem(it.id))}>{it.title}</button>
              <div className="meta">
                <span>{it.source_name}</span>
                <span className="sep">·</span>
                <span className="time">{relativeTime(it.published_at ?? it.fetched_at, now)}</span>
                {it.score !== null && <><span className="sep">·</span><span>{it.score}</span></>}
              </div>
              {it.ai_summary && <div className="summary">{it.ai_summary}</div>}
            </div>
            <button className="star" aria-pressed={it.starred} aria-label={it.starred ? "スターを外す" : "スターを付ける"}
              onClick={() => run(api.setStarred(it.id, !it.starred))}>
              <Star size={16} fill={it.starred ? "currentColor" : "none"} />
            </button>
          </article>
        ))}
      </div>
    </>
  );
}
```

`app/src/App.tsx`: `<main className="main">` の中身を次にする (`FeedView` を import):

```tsx
      <main className="main">
        <ErrorLine message={shellError} />
        {view === "feed" && (
          <FeedView selection={selection} topics={topics} status={status} view={view} onSwitchView={setView} />
        )}
        {view !== "feed" && <div className="view">{/* Task 5〜6 */}</div>}
      </main>
```

- [ ] **Step 3: テストが通ることを確認する**

Run: `cd app && npm test -- --run && npm run build`
Expected: FeedView 4 件を含めて全件 passed、ビルド成功。

- [ ] **Step 4: コミット**

```bash
git add app
git commit -m "feat(ui): add feed view with filters, read/star, and empty states"
```

---

### Task 5: ダイジェスト画面

**Files:**
- Create: `app/src/views/DigestView.tsx`, `app/src/views/DigestView.test.tsx`
- Modify: `app/src/App.tsx`

**Interfaces:**
- Consumes: `api.getDigests/regenerateDigest/getItem/retryPendingDigests`, `periodHeading/periodStart/localDateString`, `parseDigestBody/digestDisplay`, `useTauriEvent`, `EVENTS.digests`, `EVENTS.status`
- Produces: `<DigestView selection topics status view onSwitchView />`。期間切替 (今日/週/月)、見出しと日付、再生成 (全体)。各セクション: カテゴリー名 (トピック選択時はトピック名1つ)、生成元と記事数、箇条書き、段落、根拠記事 (最初の5件を `get_item` で引いてタイトルを `open_item` で開く)。状態: generating / pending (理由 + 今すぐ再試行) / failed (回数 + 理由 + 再試行) / none (記事なし)。
- サイドバーで「すべて」を選ぶとカテゴリー別 (topicId null)、カテゴリーを選ぶとそのカテゴリーだけ、トピックを選ぶとトピック別。

- [ ] **Step 1: 失敗するテストを書く**

`app/src/views/DigestView.test.tsx`:

```tsx
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { DigestView } from "./DigestView";
import { setupIpc } from "../test/ipc";
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
});
```

`vi.useFakeTimers` で `Date` だけを偽装するため、`userEvent` の内部タイマーには影響しない。もし `userEvent.click` が進まない場合は `userEvent.setup({ advanceTimers: vi.advanceTimersByTime })` を使う。

Run: `cd app && npm test -- --run`
Expected: FAIL。`./DigestView` が存在しない。

- [ ] **Step 2: 実装する**

`app/src/views/DigestView.tsx`:

```tsx
import { CircleAlert, Hourglass, Inbox, Loader, RotateCw } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { api, EVENTS } from "../api";
import { ErrorLine } from "../components/ErrorLine";
import { Segmented, ViewSwitch } from "../components/Segmented";
import type { Selection, View } from "../components/Sidebar";
import { useTauriEvent } from "../hooks/useTauriEvent";
import { localDateString, periodHeading } from "../lib/dates";
import { digestDisplay, parseDigestBody } from "../lib/digest";
import { errorMessage, type AppStatus, type DigestScope, type DigestView as DV, type Item, type Period, type Topic } from "../types";

const PERIODS: { id: Period; label: string }[] = [
  { id: "day", label: "今日" }, { id: "week", label: "週" }, { id: "month", label: "月" },
];
const REF_LIMIT = 5;

interface Props {
  selection: Selection;
  topics: Topic[];
  status: AppStatus;
  view: View;
  onSwitchView: (v: View) => void;
}

function sameScope(a: DigestScope, b: Selection): boolean {
  if (b.kind === "all") return true;
  if (b.kind === "category") return "category" in a && a.category === b.category;
  return "topic" in a && a.topic === b.topicId;
}

function References({ ids, onOpen }: { ids: number[]; onOpen: (id: number) => void }) {
  const [items, setItems] = useState<Item[]>([]);
  useEffect(() => {
    let alive = true;
    Promise.all(ids.slice(0, REF_LIMIT).map((id) => api.getItem(id).catch(() => null)))
      .then((list) => { if (alive) setItems(list.filter((i): i is Item => i !== null)); });
    return () => { alive = false; };
  }, [ids]);
  if (ids.length === 0) return null;
  return (
    <div className="digest-refs">
      <span className="text-tertiary tiny">根拠記事</span>
      {items.map((it) => <button key={it.id} className="link-btn" onClick={() => onOpen(it.id)}>{it.title}</button>)}
      {ids.length > REF_LIMIT && <span className="text-secondary">他 {ids.length - REF_LIMIT}件</span>}
    </div>
  );
}

export function DigestView({ selection, topics, status, view, onSwitchView }: Props) {
  const [period, setPeriod] = useState<Period>("day");
  const [views, setViews] = useState<DV[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const today = new Date();
  const date = localDateString(today);
  const topicId = selection.kind === "topic" ? selection.topicId : null;

  const load = useCallback(() => {
    api.getDigests(period, date, topicId)
      .then((v) => { setViews(v); setError(null); })
      .catch((e) => setError(errorMessage(e)));
  }, [period, date, topicId]);

  useEffect(load, [load]);
  useTauriEvent(EVENTS.digests, load);
  useTauriEvent(EVENTS.status, load);

  const run = (p: Promise<unknown>) => p.catch((e) => setError(errorMessage(e)));
  const heading = periodHeading(period, today);
  const shown = (views ?? []).filter((v) => sameScope(v.scope, selection));
  const scopeNote = selection.kind === "topic" ? topics.find((t) => t.id === selection.topicId)?.name ?? null : null;

  return (
    <>
      <div className="toolbar">
        <ViewSwitch view={view} onChange={onSwitchView} />
        <div className="spacer" />
        <Segmented options={PERIODS} value={period} onChange={setPeriod} ariaLabel="期間" />
      </div>
      <div className="view">
        <ErrorLine message={error} />
        <div className="view-header">
          <h1>{heading.title}</h1>
          <span className="text-secondary">{heading.range}{scopeNote ? ` · ${scopeNote}` : ""}</span>
          <div className="spacer" />
          <button className="btn btn-ghost" disabled={status.digesting}
            onClick={() => shown.forEach((v) => run(api.regenerateDigest(period, date, v.scope)))}>
            <RotateCw size={14} /> 再生成
          </button>
        </div>
        {shown.map((v) => {
          const display = digestDisplay(v, status);
          const key = "category" in v.scope ? v.scope.category : `topic-${v.scope.topic}`;
          return (
            <section key={key} className="digest-section">
              <div className="digest-head">
                <h2>{v.label}</h2>
                {display.kind === "done" && (
                  <span className="text-tertiary tiny">{display.digest.provider} · 記事{display.digest.item_ids.length}件</span>
                )}
              </div>
              {display.kind === "done" && (() => {
                const { bullets, paragraphs } = parseDigestBody(display.digest.body);
                return (
                  <>
                    <ul className="digest-bullets">{bullets.map((b, i) => <li key={i}>{b}</li>)}</ul>
                    {paragraphs.map((p, i) => <p key={i} style={{ margin: 0, lineHeight: 1.6 }}>{p}</p>)}
                    <References ids={display.digest.item_ids} onOpen={(id) => run(api.openItem(id))} />
                  </>
                );
              })()}
              {display.kind === "generating" && (
                <div className="state-box"><Loader size={15} className="spin" /><span className="grow">要約を生成中…</span></div>
              )}
              {display.kind === "pending" && (
                <div className="state-box">
                  <Hourglass size={15} />
                  <span className="grow">保留中。次回のスケジュールで再試行します。{display.error ? ` 最後のエラー: ${display.error}` : ""}</span>
                  <button className="btn" onClick={() => run(api.regenerateDigest(period, date, v.scope))}>今すぐ再試行</button>
                </div>
              )}
              {display.kind === "failed" && (
                <div className="state-box">
                  <CircleAlert size={15} style={{ color: "var(--danger)" }} />
                  <span className="grow">生成に{display.attempts}回失敗しました。LLMの設定を確認してから再試行してください。{display.error ? ` 最後のエラー: ${display.error}` : ""}</span>
                  <button className="btn" onClick={() => run(api.regenerateDigest(period, date, v.scope))}>再試行</button>
                </div>
              )}
              {display.kind === "none" && (
                <div className="state-box"><Inbox size={15} /><span className="grow">この期間に記事がありません。</span></div>
              )}
            </section>
          );
        })}
      </div>
    </>
  );
}
```

`app/src/App.tsx` のビュー部分を次にする (`DigestView` を import):

```tsx
        {view === "digest" && (
          <DigestView selection={selection} topics={topics} status={status} view={view} onSwitchView={setView} />
        )}
        {view === "feed" && (
          <FeedView selection={selection} topics={topics} status={status} view={view} onSwitchView={setView} />
        )}
        {view === "settings" && <div className="view">{/* Task 6 */}</div>}
```

- [ ] **Step 3: テストが通ることを確認する**

Run: `cd app && npm test -- --run && npm run build`
Expected: DigestView 3 件を含めて全件 passed、ビルド成功。

- [ ] **Step 4: コミット**

```bash
git add app
git commit -m "feat(ui): add digest view with period switch and generation states"
```

---

### Task 6: 設定 (ソース、RSS追加、トピック)

**Files:**
- Create: `app/src/views/SettingsView.tsx`, `app/src/views/settings/SourcesTab.tsx`, `app/src/views/settings/AddRssDialog.tsx`, `app/src/views/settings/TopicsTab.tsx`, `app/src/views/settings/SourcesTab.test.tsx`, `app/src/views/settings/TopicsTab.test.tsx`, `app/src/components/Toggle.tsx`
- Modify: `app/src/App.tsx`

**Interfaces:**
- Consumes: `api.listSources/addRssSource/updateSource/setSourceEnabled/deleteSource/listTopics/addTopic/updateTopic/deleteTopic`, `formatClock`
- Produces:
  - `<Toggle checked onChange ariaLabel />` (`role="switch"`, `aria-checked`)
  - `<SettingsView initialTab />` にタブ: ソース / トピック / LLM / スケジュール (LLM、スケジュールは Task 7)
  - `<SourcesTab />`: 一覧 (有効トグル、名前と URL、カテゴリー、最終取得、状態、編集、削除)、「RSSを追加」ボタンでダイアログ
  - `<AddRssDialog onClose onAdded />`: URL、表示名 (任意)、カテゴリー。「追加」を押すと `add_rss_source` を呼び、検証中 → 成功で `onAdded` と閉じる / 失敗でエラー表示。入力が変わるまで「追加」は無効。
  - `<TopicsTab />`: 一覧 (名前、キーワードのチップ、編集、削除)、編集カード (名前、カンマ区切りキーワード)、保存

デザインの RSS ダイアログは「検証OK」を見せてから「追加」を押す流れだが、バックエンドの `add_rss_source` は検証と保存を1回で行う。ここでは「追加」を押すと検証して保存し、失敗ならダイアログを閉じずにエラーを表示する (A-8b の検証エラー状態)。検証OKの中間状態は作らない。spec 4節「解析できない場合はエラーを表示し、保存しない」は満たす。

- [ ] **Step 1: 失敗するテストを書く**

`app/src/views/settings/SourcesTab.test.tsx`:

```tsx
import { describe, expect, it } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SourcesTab } from "./SourcesTab";
import { setupIpc } from "../../test/ipc";
import type { Source } from "../../types";

const sources: Source[] = [
  { id: 1, kind: "github_trending", name: "GitHub Trending (日)", category: "github", config: "daily", enabled: true, last_fetched_at: "2026-10-06T00:00:00Z", last_error: null },
  { id: 2, kind: "rss", name: "Zenn", category: "tech_jp", config: "https://zenn.dev/feed", enabled: true, last_fetched_at: "2026-10-06T00:00:00Z", last_error: "http: timeout" },
  { id: 3, kind: "rss", name: "The Verge", category: "tech_intl", config: "https://www.theverge.com/rss/index.xml", enabled: false, last_fetched_at: null, last_error: null },
];

describe("SourcesTab", () => {
  it("lists sources with state and toggles, deletes", async () => {
    let list = [...sources];
    const ipc = setupIpc({
      list_sources: () => list,
      set_source_enabled: (a) => { list = list.map((s) => (s.id === a.id ? { ...s, enabled: a.enabled } : s)); },
      delete_source: (a) => { list = list.filter((s) => s.id !== a.id); },
    });
    render(<SourcesTab />);
    expect(await screen.findByText("GitHub Trending (日)")).toBeInTheDocument();
    expect(screen.getByText("https://zenn.dev/feed")).toBeInTheDocument();
    expect(screen.getByText("http: timeout")).toBeInTheDocument();
    expect(screen.getByText("無効")).toBeInTheDocument();
    expect(screen.getByText(/3件のソース · 有効 2件 · エラー 1件/)).toBeInTheDocument();

    await userEvent.click(screen.getByRole("switch", { name: "The Verge を有効にする" }));
    expect(ipc.last("set_source_enabled")).toEqual({ id: 3, enabled: true });
    await waitFor(() => expect(screen.getByRole("switch", { name: /The Verge/ })).toHaveAttribute("aria-checked", "true"));

    await userEvent.click(screen.getByRole("button", { name: "Zenn を削除" }));
    expect(ipc.last("delete_source")).toEqual({ id: 2 });
    await waitFor(() => expect(screen.queryByText("Zenn")).not.toBeInTheDocument());
  });

  it("adds an RSS feed through the dialog and shows validation errors", async () => {
    // Review Focus 4
    let list = [...sources];
    const ipc = setupIpc({
      list_sources: () => list,
      add_rss_source: (a) => {
        if (a.url.includes("example.com")) throw { message: "invalid feed: no root element" };
        const s: Source = { id: 9, kind: "rss", name: a.name ?? "GIGAZINE", category: a.category, config: a.url, enabled: true, last_fetched_at: null, last_error: null };
        list = [...list, s];
        return s;
      },
    });
    render(<SourcesTab />);
    await screen.findByText("Zenn");
    await userEvent.click(screen.getByRole("button", { name: "RSSを追加" }));
    const dialog = screen.getByRole("dialog");
    const add = screen.getByRole("button", { name: "追加" });
    expect(add).toBeDisabled();

    await userEvent.type(screen.getByLabelText("フィードURL"), "https://example.com/");
    await userEvent.selectOptions(screen.getByLabelText("カテゴリー"), "tech_jp");
    expect(add).toBeEnabled();
    await userEvent.click(add);
    expect(await screen.findByText(/invalid feed: no root element/)).toBeInTheDocument();
    expect(dialog).toBeInTheDocument();
    expect(ipc.count("list_sources")).toBe(1);

    await userEvent.clear(screen.getByLabelText("フィードURL"));
    await userEvent.type(screen.getByLabelText("フィードURL"), "https://gigazine.net/news/rss_2.0/");
    await userEvent.click(screen.getByRole("button", { name: "追加" }));
    expect(ipc.last("add_rss_source")).toEqual({ url: "https://gigazine.net/news/rss_2.0/", name: null, category: "tech_jp" });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(await screen.findByText("GIGAZINE")).toBeInTheDocument();
  });

  it("edits a source inline", async () => {
    let list = [...sources];
    const ipc = setupIpc({
      list_sources: () => list,
      update_source: (a) => { list = list.map((s) => (s.id === a.id ? { ...s, name: a.name, category: a.category, config: a.config } : s)); },
    });
    render(<SourcesTab />);
    await screen.findByText("Zenn");
    await userEvent.click(screen.getByRole("button", { name: "Zenn を編集" }));
    const name = screen.getByLabelText("表示名");
    await userEvent.clear(name);
    await userEvent.type(name, "Zenn トレンド");
    await userEvent.click(screen.getByRole("button", { name: "保存" }));
    expect(ipc.last("update_source")).toEqual({ id: 2, name: "Zenn トレンド", category: "tech_jp", config: "https://zenn.dev/feed" });
    expect(await screen.findByText("Zenn トレンド")).toBeInTheDocument();
  });
});
```

`app/src/views/settings/TopicsTab.test.tsx`:

```tsx
import { describe, expect, it } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TopicsTab } from "./TopicsTab";
import { setupIpc } from "../../test/ipc";
import type { Topic } from "../../types";

describe("TopicsTab", () => {
  it("lists, adds, edits, and deletes topics", async () => {
    let list: Topic[] = [{ id: 1, name: "認知症", keywords: ["認知症", "dementia"] }];
    const ipc = setupIpc({
      list_topics: () => list,
      sidebar_counts: () => ({ total_unread: 0, categories: {}, topics: { "1": 6 } }),
      add_topic: (a) => { const t = { id: 2, name: a.name, keywords: a.keywords }; list = [...list, t]; return t; },
      update_topic: (a) => { list = list.map((t) => (t.id === a.id ? { ...t, name: a.name, keywords: a.keywords } : t)); },
      delete_topic: (a) => { list = list.filter((t) => t.id !== a.id); },
    });
    render(<TopicsTab />);
    expect(await screen.findByText("認知症")).toBeInTheDocument();
    expect(screen.getByText("dementia")).toBeInTheDocument();
    expect(screen.getByText("6")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "トピックを追加" }));
    await userEvent.type(screen.getByLabelText("名前"), "介護");
    await userEvent.type(screen.getByLabelText("キーワード"), "介護, 介護保険 ,caregiving");
    await userEvent.click(screen.getByRole("button", { name: "保存" }));
    expect(ipc.last("add_topic")).toEqual({ name: "介護", keywords: ["介護", "介護保険", "caregiving"] });
    expect(await screen.findByText("介護保険")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "認知症 を編集" }));
    const kw = screen.getByLabelText("キーワード");
    expect(kw).toHaveValue("認知症, dementia");
    await userEvent.clear(kw);
    await userEvent.type(kw, "認知症, Alzheimer");
    await userEvent.click(screen.getByRole("button", { name: "保存" }));
    expect(ipc.last("update_topic")).toEqual({ id: 1, name: "認知症", keywords: ["認知症", "Alzheimer"] });

    await userEvent.click(screen.getByRole("button", { name: "介護 を削除" }));
    expect(ipc.last("delete_topic")).toEqual({ id: 2 });
    await waitFor(() => expect(screen.queryByText("介護保険")).not.toBeInTheDocument());
  });

  it("refuses an empty name without calling the backend", async () => {
    const ipc = setupIpc({ list_topics: () => [], sidebar_counts: () => ({ total_unread: 0, categories: {}, topics: {} }) });
    render(<TopicsTab />);
    await userEvent.click(await screen.findByRole("button", { name: "トピックを追加" }));
    await userEvent.type(screen.getByLabelText("キーワード"), "x");
    expect(screen.getByRole("button", { name: "保存" })).toBeDisabled();
    expect(ipc.count("add_topic")).toBe(0);
  });
});
```

Run: `cd app && npm test -- --run`
Expected: FAIL。`./SourcesTab`、`./TopicsTab` が存在しない。

- [ ] **Step 2: 実装する**

`app/src/components/Toggle.tsx`:

```tsx
export function Toggle({ checked, onChange, ariaLabel }: { checked: boolean; onChange: (v: boolean) => void; ariaLabel: string }) {
  return <button className="toggle" role="switch" aria-checked={checked} aria-label={ariaLabel} onClick={() => onChange(!checked)} />;
}
```

`app/src/views/settings/AddRssDialog.tsx`:

```tsx
import { CircleAlert, Loader } from "lucide-react";
import { useState } from "react";
import { api } from "../../api";
import { CATEGORIES, errorMessage, type Category, type Source } from "../../types";

export function AddRssDialog({ onClose, onAdded }: { onClose: () => void; onAdded: (s: Source) => void }) {
  const [url, setUrl] = useState("");
  const [name, setName] = useState("");
  const [category, setCategory] = useState<Category>("tech_jp");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const canSubmit = url.trim().length > 0 && !busy;

  const submit = () => {
    setBusy(true);
    setError(null);
    api.addRssSource(url.trim(), name.trim() ? name.trim() : null, category)
      .then((s) => { onAdded(s); onClose(); })
      .catch((e) => { setError(errorMessage(e)); setBusy(false); });
  };

  return (
    <div className="overlay" onClick={onClose}>
      <div className="dialog" role="dialog" aria-label="RSSフィードを追加" onClick={(e) => e.stopPropagation()}>
        <h2>RSSフィードを追加</h2>
        <div className="row">
          <label htmlFor="rss-url">フィードURL</label>
          <input id="rss-url" className={`field ${error ? "field-error" : ""}`} value={url} onChange={(e) => { setUrl(e.target.value); setError(null); }} placeholder="https://" />
        </div>
        <div className="row">
          <label htmlFor="rss-name">表示名 (任意)</label>
          <input id="rss-name" className="field" value={name} onChange={(e) => setName(e.target.value)} placeholder="空欄の場合はフィードのタイトルを使います" />
        </div>
        <div className="row">
          <label htmlFor="rss-category">カテゴリー</label>
          <select id="rss-category" className="field" value={category} onChange={(e) => setCategory(e.target.value as Category)}>
            {CATEGORIES.map((c) => <option key={c.id} value={c.id}>{c.label}</option>)}
          </select>
        </div>
        <div className={`validation ${error ? "error" : ""}`} aria-live="polite">
          {busy && <><Loader size={14} className="spin" /><span>フィードを取得して検証しています…</span></>}
          {error && <><CircleAlert size={14} /><span>{error} このURLは保存されません。</span></>}
        </div>
        <div className="actions">
          <button className="btn" onClick={onClose}>キャンセル</button>
          <button className="btn btn-primary" disabled={!canSubmit} onClick={submit}>追加</button>
        </div>
      </div>
    </div>
  );
}
```

`app/src/views/settings/SourcesTab.tsx`:

```tsx
import { Pencil, Plus, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { api } from "../../api";
import { ErrorLine } from "../../components/ErrorLine";
import { Toggle } from "../../components/Toggle";
import { formatClock } from "../../lib/dates";
import { CATEGORIES, categoryLabel, errorMessage, type Category, type Source } from "../../types";
import { AddRssDialog } from "./AddRssDialog";

function statusCell(s: Source) {
  if (!s.enabled) return <span className="cell-status"><i className="dot dot-muted" />無効</span>;
  if (s.last_error) return <span className="cell-status error"><i className="dot dot-danger" />{s.last_error}</span>;
  if (s.last_fetched_at) return <span className="cell-status"><i className="dot dot-success" />取得済み</span>;
  return <span className="cell-status"><i className="dot dot-muted" />未取得</span>;
}

export function SourcesTab() {
  const [sources, setSources] = useState<Source[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [editing, setEditing] = useState<Source | null>(null);

  const load = useCallback(() => {
    api.listSources().then((s) => { setSources(s); setError(null); }).catch((e) => setError(errorMessage(e)));
  }, []);
  useEffect(load, [load]);
  const run = (p: Promise<unknown>) => p.then(load).catch((e) => setError(errorMessage(e)));

  const enabled = sources.filter((s) => s.enabled).length;
  const errors = sources.filter((s) => s.enabled && s.last_error).length;

  return (
    <div className="section">
      <div style={{ display: "flex", alignItems: "center" }}>
        <h2>ソース</h2>
        <div style={{ flex: 1 }} />
        <button className="btn btn-primary" onClick={() => setAdding(true)}><Plus size={14} /> RSSを追加</button>
      </div>
      <ErrorLine message={error} />
      <div className="table-head sources-grid"><span /><span>ソース</span><span>カテゴリー</span><span>最終取得</span><span>状態</span><span /></div>
      {sources.map((s) => (
        <div key={s.id} className="table-row sources-grid">
          <Toggle checked={s.enabled} ariaLabel={`${s.name} を${s.enabled ? "無効" : "有効"}にする`} onChange={(v) => run(api.setSourceEnabled(s.id, v))} />
          <div className="cell-name">
            <span style={{ fontWeight: 500, color: s.enabled ? undefined : "var(--text-tertiary)" }}>{s.name}</span>
            <span className="url">{s.config}</span>
          </div>
          <span className="small text-secondary">{categoryLabel(s.category)}</span>
          <span className="small text-secondary">{s.last_fetched_at ? formatClock(s.last_fetched_at) : "—"}</span>
          {statusCell(s)}
          <div className="cell-actions">
            <button aria-label={`${s.name} を編集`} onClick={() => setEditing(s)}><Pencil size={14} /></button>
            <button aria-label={`${s.name} を削除`} onClick={() => run(api.deleteSource(s.id))}><Trash2 size={14} /></button>
          </div>
        </div>
      ))}
      <div className="small text-tertiary" style={{ padding: "12px 0" }}>{sources.length}件のソース · 有効 {enabled}件 · エラー {errors}件</div>
      {editing && <EditSourceCard source={editing} onClose={() => setEditing(null)} onSaved={() => { setEditing(null); load(); }} onError={setError} />}
      {adding && <AddRssDialog onClose={() => setAdding(false)} onAdded={() => load()} />}
    </div>
  );
}

function EditSourceCard({ source, onClose, onSaved, onError }: { source: Source; onClose: () => void; onSaved: () => void; onError: (m: string) => void }) {
  const [name, setName] = useState(source.name);
  const [category, setCategory] = useState<Category>(source.category);
  const [config, setConfig] = useState(source.config);
  const isRss = source.kind === "rss";
  return (
    <div className="card">
      <h2 style={{ fontSize: 13 }}>ソースを編集</h2>
      <div className="row"><label htmlFor="src-name">表示名</label><input id="src-name" className="field" value={name} onChange={(e) => setName(e.target.value)} /></div>
      <div className="row"><label htmlFor="src-cat">カテゴリー</label>
        <select id="src-cat" className="field" value={category} onChange={(e) => setCategory(e.target.value as Category)}>
          {CATEGORIES.map((c) => <option key={c.id} value={c.id}>{c.label}</option>)}
        </select>
      </div>
      <div className="row"><label htmlFor="src-config">{isRss ? "フィードURL" : "設定値"}</label><input id="src-config" className="field" value={config} onChange={(e) => setConfig(e.target.value)} /></div>
      <div className="actions">
        <button className="btn" onClick={onClose}>キャンセル</button>
        <button className="btn btn-primary" disabled={!name.trim() || !config.trim()}
          onClick={() => api.updateSource(source.id, name.trim(), category, config.trim()).then(onSaved).catch((e) => onError(errorMessage(e)))}>保存</button>
      </div>
    </div>
  );
}
```

`app/src/views/settings/TopicsTab.tsx`:

```tsx
import { Hash, Pencil, Plus, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { api } from "../../api";
import { ErrorLine } from "../../components/ErrorLine";
import { errorMessage, type SidebarCounts, type Topic } from "../../types";

type Draft = { id: number | null; name: string; keywords: string };

function splitKeywords(s: string): string[] {
  return s.split(/[,、]/).map((k) => k.trim()).filter((k) => k.length > 0);
}

export function TopicsTab() {
  const [topics, setTopics] = useState<Topic[]>([]);
  const [counts, setCounts] = useState<SidebarCounts | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    Promise.all([api.listTopics(), api.sidebarCounts()])
      .then(([t, c]) => { setTopics(t); setCounts(c); setError(null); })
      .catch((e) => setError(errorMessage(e)));
  }, []);
  useEffect(load, [load]);
  const run = (p: Promise<unknown>) => p.then(load).catch((e) => setError(errorMessage(e)));

  const save = () => {
    if (!draft) return;
    const keywords = splitKeywords(draft.keywords);
    const p = draft.id === null ? api.addTopic(draft.name.trim(), keywords) : api.updateTopic(draft.id, draft.name.trim(), keywords);
    p.then(() => { setDraft(null); load(); }).catch((e) => setError(errorMessage(e)));
  };

  return (
    <div className="section">
      <div style={{ display: "flex", alignItems: "center" }}>
        <h2>トピック</h2>
        <div style={{ flex: 1 }} />
        <button className="btn btn-primary" onClick={() => setDraft({ id: null, name: "", keywords: "" })}><Plus size={14} /> トピックを追加</button>
      </div>
      <p className="desc">キーワードのいずれかを含む記事がトピックに属します。日本語と英語を併記すると両方の記事が集まります。英数字だけの語は単語単位で照合します。</p>
      <ErrorLine message={error} />
      <div className="table-head topics-grid"><span>トピック</span><span>キーワード</span><span>未読</span><span /></div>
      {topics.map((t) => (
        <div key={t.id} className="table-row topics-grid">
          <span style={{ display: "flex", gap: 8, alignItems: "center", fontWeight: 500 }}><Hash size={14} className="text-secondary" />{t.name}</span>
          <span style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>{t.keywords.map((k) => <span key={k} className="chip">{k}</span>)}</span>
          <span className="small text-secondary">{counts?.topics[String(t.id)] ?? 0}</span>
          <div className="cell-actions">
            <button aria-label={`${t.name} を編集`} onClick={() => setDraft({ id: t.id, name: t.name, keywords: t.keywords.join(", ") })}><Pencil size={14} /></button>
            <button aria-label={`${t.name} を削除`} onClick={() => run(api.deleteTopic(t.id))}><Trash2 size={14} /></button>
          </div>
        </div>
      ))}
      {draft && (
        <div className="card">
          <h2 style={{ fontSize: 13 }}>{draft.id === null ? "トピックを追加" : "トピックを編集"}</h2>
          <div className="row"><label htmlFor="topic-name">名前</label><input id="topic-name" className="field" value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} /></div>
          <div className="row" style={{ alignItems: "flex-start" }}>
            <label htmlFor="topic-kw" style={{ paddingTop: 6 }}>キーワード</label>
            <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: 6 }}>
              <input id="topic-kw" className="field" value={draft.keywords} onChange={(e) => setDraft({ ...draft, keywords: e.target.value })} placeholder="認知症, dementia, Alzheimer" />
              <span className="tiny text-tertiary">カンマ区切り。2文字以下の語も部分一致で照合します。</span>
            </div>
          </div>
          <div className="actions">
            <button className="btn" onClick={() => setDraft(null)}>キャンセル</button>
            <button className="btn btn-primary" disabled={!draft.name.trim()} onClick={save}>保存</button>
          </div>
        </div>
      )}
    </div>
  );
}
```

`app/src/views/SettingsView.tsx`:

```tsx
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
```

`app/src/App.tsx`: `{view === "settings" && <SettingsView />}` に差し替える (import を追加)。

- [ ] **Step 3: テストが通ることを確認する**

Run: `cd app && npm test -- --run && npm run build`
Expected: Sources 3 件、Topics 2 件を含めて全件 passed、ビルド成功。

- [ ] **Step 4: コミット**

```bash
git add app
git commit -m "feat(ui): add settings view with sources, RSS dialog, and topics"
```

---

### Task 7: 設定 (LLM、スケジュール)

**Files:**
- Create: `app/src/views/settings/LlmTab.tsx`, `app/src/views/settings/ScheduleTab.tsx`, `app/src/views/settings/LlmTab.test.tsx`, `app/src/views/settings/ScheduleTab.test.tsx`
- Modify: `app/src/views/SettingsView.tsx`

**Interfaces:**
- Consumes: `api.getSettings/setSetting/testOllama/testClaude/setLaunchAtLogin/collectNow/retryPendingDigests/getStatus`, 設定キー (Global Constraints)
- Produces:
  - `<LlmTab />`: 記事要約 / 期間要約 / チャットのプロバイダー (`llm.items_provider`、`llm.digest_provider`、`llm.chat_provider`: `ollama` | `claude`)、Ollama (`ollama.base_url`、`ollama.model`、接続テスト、`ollama.web_search_api_key`)、Claude (`claude.bin`、接続テスト)、Node (`node.bin`)。テキスト欄は blur または Enter で保存。
  - `<ScheduleTab status />`: 収集間隔 (`schedule.collect_interval_hours`: 1/3/6/12)、次回の収集 (status.next_collect_at)、「今すぐ収集」、日次時刻 (`schedule.daily_time`)、週次 (`schedule.weekly_day`: mon〜sun、`schedule.weekly_time`)、月次 (`schedule.monthly_time`)、ログイン時に起動 (`app.launch_at_login`、`set_launch_at_login` 経由)、閉じても常駐 (`app.stay_resident`)、保留中の要約の「今すぐ再試行」。
  - `SettingsView` は `status` を受け取り、`ScheduleTab` に渡す。

- [ ] **Step 1: 失敗するテストを書く**

`app/src/views/settings/LlmTab.test.tsx`:

```tsx
import { describe, expect, it } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { LlmTab } from "./LlmTab";
import { setupIpc } from "../../test/ipc";

const settings = {
  "llm.items_provider": "ollama", "llm.digest_provider": "claude", "llm.chat_provider": "ollama",
  "ollama.base_url": "http://localhost:11434", "ollama.model": "gemma4:26b", "ollama.web_search_api_key": "",
  "claude.bin": "claude", "node.bin": "node", "summary.top_n": "20",
};

describe("LlmTab", () => {
  it("shows providers and saves changes", async () => {
    const ipc = setupIpc({ get_settings: () => settings, set_setting: () => undefined });
    render(<LlmTab />);
    expect(await screen.findByLabelText("期間要約")).toHaveValue("claude");
    await userEvent.selectOptions(screen.getByLabelText("記事要約"), "claude");
    expect(ipc.last("set_setting")).toEqual({ key: "llm.items_provider", value: "claude" });

    const model = screen.getByLabelText("モデル");
    await userEvent.clear(model);
    await userEvent.type(model, "gemma4:12b{enter}");
    expect(ipc.last("set_setting")).toEqual({ key: "ollama.model", value: "gemma4:12b" });
  });

  it("runs connection tests and shows the result or the error", async () => {
    setupIpc({
      get_settings: () => settings,
      test_ollama: () => ({ ok: true, message: "接続済み (http://localhost:11434/api/tags)", millis: 800 }),
      test_claude: () => ({ ok: false, message: "claude: Not logged in", millis: 5000 }),
    });
    render(<LlmTab />);
    await screen.findByLabelText("モデル");
    await userEvent.click(screen.getByRole("button", { name: "Ollama をテスト" }));
    expect(await screen.findByText(/接続済み/)).toBeInTheDocument();
    expect(screen.getByText(/0\.8秒/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Claude をテスト" }));
    expect(await screen.findByText(/Not logged in/)).toBeInTheDocument();
  });

  it("shows the backend validation error for a bad path", async () => {
    setupIpc({
      get_settings: () => settings,
      set_setting: (a) => { if (a.key === "claude.bin") throw { message: "claude.bin: not an existing file" }; },
    });
    render(<LlmTab />);
    const bin = await screen.findByLabelText("claude コマンドのパス");
    await userEvent.clear(bin);
    await userEvent.type(bin, "/nope/claude{enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent("not an existing file");
  });
});
```

`app/src/views/settings/ScheduleTab.test.tsx`:

```tsx
import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ScheduleTab } from "./ScheduleTab";
import { setupIpc } from "../../test/ipc";
import type { AppStatus } from "../../types";

const settings = {
  "schedule.collect_interval_hours": "3", "schedule.daily_time": "07:00", "schedule.weekly_day": "mon",
  "schedule.weekly_time": "07:30", "schedule.monthly_time": "08:00", "app.launch_at_login": "false", "app.stay_resident": "true",
};
const status: AppStatus = { collecting: false, summarizing: false, digesting: false, last_collect_at: null, next_collect_at: "2026-10-06T03:00:00Z", last_error: null };

describe("ScheduleTab", () => {
  it("shows schedule settings and saves them", async () => {
    const ipc = setupIpc({ get_settings: () => settings, set_setting: () => undefined, set_launch_at_login: () => undefined, collect_now: () => undefined, retry_pending_digests: () => undefined });
    render(<ScheduleTab status={status} />);
    expect(await screen.findByLabelText("収集間隔")).toHaveValue("3");
    expect(screen.getByText(/次回の収集/)).toBeInTheDocument();

    await userEvent.selectOptions(screen.getByLabelText("収集間隔"), "6");
    expect(ipc.last("set_setting")).toEqual({ key: "schedule.collect_interval_hours", value: "6" });

    const daily = screen.getByLabelText("日次要約の時刻");
    await userEvent.clear(daily);
    await userEvent.type(daily, "06:30{enter}");
    expect(ipc.last("set_setting")).toEqual({ key: "schedule.daily_time", value: "06:30" });

    await userEvent.selectOptions(screen.getByLabelText("週次要約の曜日"), "sun");
    expect(ipc.last("set_setting")).toEqual({ key: "schedule.weekly_day", value: "sun" });

    await userEvent.click(screen.getByRole("switch", { name: "ログイン時に起動" }));
    expect(ipc.last("set_launch_at_login")).toEqual({ enabled: true });
    expect(ipc.count("set_setting")).toBe(3);

    await userEvent.click(screen.getByRole("switch", { name: "ウィンドウを閉じても常駐する" }));
    expect(ipc.last("set_setting")).toEqual({ key: "app.stay_resident", value: "false" });

    await userEvent.click(screen.getByRole("button", { name: "今すぐ収集" }));
    expect(ipc.count("collect_now")).toBe(1);
    await userEvent.click(screen.getByRole("button", { name: "保留中の要約を再試行" }));
    expect(ipc.count("retry_pending_digests")).toBe(1);
  });
});
```

Run: `cd app && npm test -- --run`
Expected: FAIL。`./LlmTab`、`./ScheduleTab` が存在しない。

- [ ] **Step 2: 実装する**

`app/src/views/settings/LlmTab.tsx`:

```tsx
import { useEffect, useState } from "react";
import { api } from "../../api";
import { ErrorLine } from "../../components/ErrorLine";
import { errorMessage, type ProbeResult, type Settings } from "../../types";

const PROVIDERS = [{ id: "ollama", label: "Ollama" }, { id: "claude", label: "Claude (claude CLI)" }];

/** blur または Enter で保存するテキスト欄。 */
export function SettingField({ id, label, value, onSave, hint, type = "text", width = 260 }: {
  id: string; label: string; value: string; onSave: (v: string) => void; hint?: string; type?: string; width?: number;
}) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const commit = () => { if (draft !== value) onSave(draft); };
  return (
    <div className="setting-row">
      <div className="label"><label htmlFor={id}>{label}</label>{hint && <span className="hint">{hint}</span>}</div>
      <input id={id} type={type} className="field" style={{ width }} value={draft}
        onChange={(e) => setDraft(e.target.value)} onBlur={commit}
        onKeyDown={(e) => { if (e.key === "Enter") commit(); }} />
    </div>
  );
}

function ProviderRow({ id, label, hint, value, onChange }: { id: string; label: string; hint: string; value: string; onChange: (v: string) => void }) {
  return (
    <div className="setting-row">
      <div className="label"><label htmlFor={id}>{label}</label><span className="hint">{hint}</span></div>
      <select id={id} className="field" style={{ width: 180 }} value={value} onChange={(e) => onChange(e.target.value)}>
        {PROVIDERS.map((p) => <option key={p.id} value={p.id}>{p.label}</option>)}
      </select>
    </div>
  );
}

function Probe({ label, run }: { label: string; run: () => Promise<ProbeResult> }) {
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<ProbeResult | null>(null);
  return (
    <div className="setting-row">
      <div className="label"><span>接続テスト</span></div>
      {result && (
        <span className={`cell-status ${result.ok ? "" : "error"}`}>
          <i className={`dot ${result.ok ? "dot-success" : "dot-danger"}`} />
          {result.message} · {(result.millis / 1000).toFixed(1)}秒
        </span>
      )}
      <button className="btn" disabled={busy} onClick={() => { setBusy(true); run().then(setResult).catch((e) => setResult({ ok: false, message: errorMessage(e), millis: 0 })).finally(() => setBusy(false)); }}>
        {busy ? "テスト中…" : `${label} をテスト`}
      </button>
    </div>
  );
}

export function LlmTab() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = () => api.getSettings().then((s) => { setSettings(s); setError(null); }).catch((e) => setError(errorMessage(e)));
  useEffect(() => { load(); }, []);
  const save = (key: string, value: string) =>
    api.setSetting(key, value).then(load).catch((e) => setError(errorMessage(e)));
  if (!settings) return <ErrorLine message={error} />;
  const g = (k: string) => settings[k] ?? "";
  return (
    <>
      <ErrorLine message={error} />
      <section className="section">
        <h2>要約に使うLLM</h2>
        <p className="desc">記事要約と期間要約で別のプロバイダーを選べます。Claudeを選んでも、失敗時にOllamaへ自動で切り替えません。</p>
        <ProviderRow id="llm-items" label="記事要約" hint="収集のたびにカテゴリーごと上位N件を要約" value={g("llm.items_provider")} onChange={(v) => save("llm.items_provider", v)} />
        <ProviderRow id="llm-digest" label="期間要約" hint="日次、週次、月次のダイジェスト" value={g("llm.digest_provider")} onChange={(v) => save("llm.digest_provider", v)} />
        <ProviderRow id="llm-chat" label="チャット" hint="エージェントの既定。パネル内で切替可" value={g("llm.chat_provider")} onChange={(v) => save("llm.chat_provider", v)} />
      </section>
      <section className="section">
        <h2>Ollama</h2>
        <SettingField id="ollama-url" label="URL" value={g("ollama.base_url")} onSave={(v) => save("ollama.base_url", v)} />
        <SettingField id="ollama-model" label="モデル" value={g("ollama.model")} onSave={(v) => save("ollama.model", v)} />
        <Probe label="Ollama" run={api.testOllama} />
        <SettingField id="ollama-key" label="Web検索 APIキー" type="password" hint="ollama.com のAPIキー。未設定ならチャットのWeb検索を無効にする" value={g("ollama.web_search_api_key")} onSave={(v) => save("ollama.web_search_api_key", v)} />
      </section>
      <section className="section">
        <h2>Claude</h2>
        <SettingField id="claude-bin" label="claude コマンドのパス" hint="GUIアプリはシェルのPATHを引き継がないため、絶対パスを指定" value={g("claude.bin")} onSave={(v) => save("claude.bin", v)} />
        <Probe label="Claude" run={api.testClaude} />
        <SettingField id="node-bin" label="Node のパス" hint="チャットのエージェント実行に使用 (22以上)" value={g("node.bin")} onSave={(v) => save("node.bin", v)} />
      </section>
    </>
  );
}
```

`app/src/views/settings/ScheduleTab.tsx`:

```tsx
import { useEffect, useState } from "react";
import { api } from "../../api";
import { ErrorLine } from "../../components/ErrorLine";
import { Toggle } from "../../components/Toggle";
import { formatClock } from "../../lib/dates";
import { errorMessage, type AppStatus, type Settings } from "../../types";
import { SettingField } from "./LlmTab";

const INTERVALS = ["1", "3", "6", "12"];
const WEEKDAYS = [["mon", "月曜"], ["tue", "火曜"], ["wed", "水曜"], ["thu", "木曜"], ["fri", "金曜"], ["sat", "土曜"], ["sun", "日曜"]];

export function ScheduleTab({ status }: { status: AppStatus }) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = () => api.getSettings().then((s) => { setSettings(s); setError(null); }).catch((e) => setError(errorMessage(e)));
  useEffect(() => { load(); }, []);
  const save = (key: string, value: string) => api.setSetting(key, value).then(load).catch((e) => setError(errorMessage(e)));
  const run = (p: Promise<unknown>) => p.then(load).catch((e) => setError(errorMessage(e)));
  if (!settings) return <ErrorLine message={error} />;
  const g = (k: string) => settings[k] ?? "";
  return (
    <>
      <ErrorLine message={error} />
      <section className="section">
        <h2>収集</h2>
        <div className="setting-row">
          <div className="label"><label htmlFor="interval">収集間隔</label></div>
          <select id="interval" className="field" style={{ width: 180 }} value={g("schedule.collect_interval_hours")} onChange={(e) => save("schedule.collect_interval_hours", e.target.value)}>
            {INTERVALS.map((h) => <option key={h} value={h}>{h}時間おき</option>)}
          </select>
        </div>
        <div className="setting-row">
          <div className="label"><span>次回の収集</span><span className="hint">手動で今すぐ収集もできます</span></div>
          <span className="small text-secondary">{status.next_collect_at ? formatClock(status.next_collect_at) : "—"}</span>
          <button className="btn" disabled={status.collecting} onClick={() => run(api.collectNow())}>今すぐ収集</button>
        </div>
      </section>
      <section className="section">
        <h2>要約の生成</h2>
        <p className="desc">日次の時刻に前日の日次要約を確定し、現在の週次と月次を作り直します。当日の日次要約は収集のたびに更新されます。</p>
        <SettingField id="daily-time" label="日次要約の時刻" width={120} hint="HH:MM" value={g("schedule.daily_time")} onSave={(v) => save("schedule.daily_time", v)} />
        <div className="setting-row">
          <div className="label"><label htmlFor="weekly-day">週次要約の曜日</label><span className="hint">日次要約から生成</span></div>
          <select id="weekly-day" className="field" style={{ width: 120 }} value={g("schedule.weekly_day")} onChange={(e) => save("schedule.weekly_day", e.target.value)}>
            {WEEKDAYS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
          </select>
        </div>
        <SettingField id="weekly-time" label="週次要約の時刻" width={120} value={g("schedule.weekly_time")} onSave={(v) => save("schedule.weekly_time", v)} />
        <SettingField id="monthly-time" label="月次要約の時刻 (毎月1日)" width={120} hint="HH:MM。週次要約から生成" value={g("schedule.monthly_time")} onSave={(v) => save("schedule.monthly_time", v)} />
      </section>
      <section className="section">
        <h2>起動</h2>
        <div className="setting-row">
          <div className="label"><span>ログイン時に起動</span><span className="hint">メニューバーに常駐し、ウィンドウは開きません</span></div>
          <Toggle checked={g("app.launch_at_login") === "true"} ariaLabel="ログイン時に起動" onChange={(v) => run(api.setLaunchAtLogin(v))} />
        </div>
        <div className="setting-row">
          <div className="label"><span>ウィンドウを閉じても常駐する</span><span className="hint">オフにすると閉じた時に終了します</span></div>
          <Toggle checked={g("app.stay_resident") !== "false"} ariaLabel="ウィンドウを閉じても常駐する" onChange={(v) => save("app.stay_resident", v ? "true" : "false")} />
        </div>
      </section>
      <section className="section">
        <h2>保留中の要約</h2>
        <p className="desc">LLMに接続できず生成できなかった要約は、次回のスケジュールで再試行します。手動で今すぐ再試行することもできます (失敗した要約も含みます)。</p>
        <div className="setting-row">
          <div className="label"><span>{status.last_error ? `最後のエラー: ${status.last_error}` : "エラーはありません"}</span></div>
          <button className="btn" onClick={() => run(api.retryPendingDigests())}>保留中の要約を再試行</button>
        </div>
      </section>
    </>
  );
}
```

時刻はテキスト欄 (`HH:MM`)。バックエンドは `HH:MM` 以外の値を既定値に戻す。

`app/src/views/SettingsView.tsx`: props に `status: AppStatus` を追加し、`{tab === "llm" && <LlmTab />}`、`{tab === "schedule" && <ScheduleTab status={status} />}` に差し替える。`App.tsx` は `<SettingsView status={status} />` を渡す。

- [ ] **Step 3: テストが通ることを確認する**

Run: `cd app && npm test -- --run && npm run build`
Expected: LlmTab 3 件、ScheduleTab 1 件を含めて全件 passed、ビルド成功。

- [ ] **Step 4: コミット**

```bash
git add app
git commit -m "feat(ui): add LLM and schedule settings tabs"
```

---

### Task 8: 実機確認、ダークモード、README

**Files:**
- Modify: `README.md`, 必要なら `app/src/styles/*.css`

**Interfaces:**
- Consumes: Task 1〜7 の全部
- Produces: `npm run tauri build -- --debug --bundles app` で作った `InfoDesk.app` が、デザイン A-1/A-2/A-3/A-5/A-6/A-7/A-8/A-10/A-11 のとおりに動く状態

- [ ] **Step 1: ビルドして起動する**

```bash
cd app && npm run tauri build -- --debug --bundles app && open ../target/debug/bundle/macos/InfoDesk.app
```

- [ ] **Step 2: 確認する (手動、画面)**

デザインの書き出し (`design/exports/`) と並べて確認する。各項目で違いがあれば直してから再ビルドする。

1. 起動直後: サイドバーに7カテゴリーと未読件数、左下に「最終収集 HH:MM」。ダイジェスト (今日) にカテゴリー別の要約。要約がないカテゴリーは「この期間に記事がありません。」または保留/失敗の表示 (A-10)。
2. 「週」「月」に切り替えると見出しと日付範囲が変わり、内容が入れ替わる。
3. フィード: 未読の青丸、タイトルをクリックすると既定ブラウザで開き、行が既読 (細字) になる。スターの付け外し。期間、ソース、未読のみ、検索が効く。「すべて既読にする」でサイドバーの件数が0になる。
4. サイドバーで「医療介護」を選ぶとフィードとダイジェストがそのカテゴリーだけになる。トピックを選ぶとトピック別の要約と記事。
5. 設定 → ソース: 一覧、トグル、編集、削除。「RSSを追加」で `https://example.com/` を入れるとエラー表示で保存されない。有効な RSS を入れると一覧に追加され、次の収集で記事が入る。
6. 設定 → トピック: 追加、編集、削除。サイドバーに反映される。
7. 設定 → LLM: Ollama の接続テストが「接続済み」。Claude の接続テストが成功または理由付きで失敗。モデル名を変えて保存される。
8. 設定 → スケジュール: 値の変更が保存される。「ログイン時に起動」の切替。「ウィンドウを閉じても常駐する」をオフにして閉じるとアプリが終了する (確認後に戻す)。
9. システム設定で外観をダークにすると全画面がダーク配色になる (A-1〜A-3 のダーク版)。
10. ウィンドウを 960x640 に縮めても横スクロールが出ず、ツールバーが折り返しなしで収まる。収まらない場合は `.toolbar` の検索欄の幅を縮める。
11. メニューバーの「設定…」でウィンドウが開き設定ビューになる。

- [ ] **Step 3: README を更新する**

`README.md` の「## アプリ」の末尾に追加:

````markdown
画面は `app/src` (React)。テストは `cd app && npm test`。デザインは `design.pen` と `design/exports/`。
````

- [ ] **Step 4: コミット**

```bash
git add app README.md
git commit -m "feat(ui): verify screens against the design on macOS"
```

確認した結果 (各項目の実際の挙動) をコミットメッセージの本文に書く。

---

## Plan 3 (チャット) への引き継ぎ

- ツールバー右端にチャット開閉ボタンを追加し、`App.tsx` の `main` を「ビュー + 右パネル」の横並びにする (A-4)。
- 記事行の右クリックまたはホバーに「チャットで質問」を追加する。
- `SettingsView` の LLM タブには既に `llm.chat_provider`、`ollama.web_search_api_key`、`node.bin` がある。
