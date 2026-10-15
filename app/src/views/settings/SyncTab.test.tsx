import { describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SyncTab } from "./SyncTab";
import { useRef } from "react";
import { useShortcuts, type ViewActions } from "../../hooks/useShortcuts";
import { emitEvent, setupIpc } from "../../test/ipc";
import { EVENTS } from "../../api";
import type { SyncStatus } from "../../types";

const out: SyncStatus = {
  available: true, signed_in: false, account_email: null, enabled: false, syncing: false,
  last_sync_at: null, error: null, device_name: "MacBook Pro",
  backend: "gdrive", nas_url: "", nas_user: "", nas_password_set: false,
  tailnet: { state: "stopped", name: null, reason: null },
};
const nas: SyncStatus = {
  ...out, backend: "nas", enabled: true, nas_url: "http://takeuchi-nas:5005/infodesk/", nas_user: "infodesk", nas_password_set: true,
  tailnet: { state: "running", name: "infodesk-mac", reason: null },
};
const signedIn: SyncStatus = { ...out, signed_in: true, account_email: "you@gmail.com", enabled: true, last_sync_at: "2026-10-06T03:00:00Z" };

function deferred<T = SyncStatus>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => { resolve = r; });
  return { promise, resolve };
}

describe("SyncTab", () => {
  it("shows the signed-out state", async () => {
    const ipc = setupIpc({ sync_status: () => out });
    render(<SyncTab />);
    expect(await screen.findByRole("button", { name: "Google でログイン" })).toBeEnabled();
    expect(screen.getByText("収集した記事とダイジェストを置く場所。スマホの InfoDesk も同じ場所から読めます。")).toBeInTheDocument();
    expect(screen.getByText("ログインすると、ブラウザで Google の画面が開きます。ID やパスワードはアプリに入力しません。")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "同期する" })).toBeDisabled();
    expect(screen.getByText("記事と日次ダイジェストは直近60日、週次と月次のダイジェストは直近400日。スマホの控えは消えません。")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "ログアウト" })).not.toBeInTheDocument();
    expect(ipc.unmocked).toEqual([]);
  });

  it("shows the signed-in state", async () => {
    setupIpc({ sync_status: () => signedIn });
    render(<SyncTab />);
    expect(await screen.findByText("you@gmail.com でログイン中")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "ログアウト" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Google でログイン" })).not.toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "同期する" })).toBeEnabled();
    expect(screen.getByRole("switch", { name: "同期する" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByLabelText("この Mac の名前")).toHaveValue("MacBook Pro");
    expect(screen.getByTestId("last-sync").textContent).toMatch(/^\d{2}:\d{2}$/);
  });

  it("shows only the unavailable notice when the build has no OAuth config", async () => {
    setupIpc({ sync_status: () => ({ ...out, available: false }) });
    render(<SyncTab />);
    expect(await screen.findByText("このビルドでは同期を使えません")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Google でログイン" })).not.toBeInTheDocument();
    expect(within(screen.getByRole("group", { name: "保存先" })).getByRole("button", { name: "NAS (WebDAV)" })).toBeInTheDocument();
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("この Mac の名前")).not.toBeInTheDocument();
  });

  it("calls sync_login and disables the button while the browser is open", async () => {
    const login = deferred();
    const ipc = setupIpc({ sync_status: () => out, sync_login: () => login.promise });
    render(<SyncTab />);
    await userEvent.click(await screen.findByRole("button", { name: "Google でログイン" }));
    expect(ipc.count("sync_login")).toBe(1);
    const busy = screen.getByRole("button", { name: "ブラウザでログインしてください…" });
    expect(busy).toBeDisabled();
    await act(async () => login.resolve(signedIn));
    expect(await screen.findByText("you@gmail.com でログイン中")).toBeInTheDocument();
  });

  it("shows a login failure and re-enables the button", async () => {
    setupIpc({ sync_status: () => out, sync_login: () => { throw { message: "タイムアウトしました" }; } });
    render(<SyncTab />);
    await userEvent.click(await screen.findByRole("button", { name: "Google でログイン" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("タイムアウトしました");
    expect(screen.getByRole("button", { name: "Google でログイン" })).toBeEnabled();
  });

  it("logs out", async () => {
    const ipc = setupIpc({ sync_status: () => signedIn, sync_logout: () => out });
    render(<SyncTab />);
    await userEvent.click(await screen.findByRole("button", { name: "ログアウト" }));
    expect(ipc.count("sync_logout")).toBe(1);
    expect(await screen.findByRole("button", { name: "Google でログイン" })).toBeInTheDocument();
  });

  it("toggles sync with sync_set_enabled", async () => {
    const ipc = setupIpc({ sync_status: () => signedIn, sync_set_enabled: ({ enabled }) => ({ ...signedIn, enabled }) });
    render(<SyncTab />);
    await userEvent.click(await screen.findByRole("switch", { name: "同期する" }));
    expect(ipc.last("sync_set_enabled")).toEqual({ enabled: false });
    await waitFor(() => expect(screen.getByRole("switch", { name: "同期する" })).toHaveAttribute("aria-checked", "false"));
  });

  it("saves the device name on Enter and on blur, not during IME composition", async () => {
    const ipc = setupIpc({ sync_status: () => signedIn, sync_set_device_name: ({ name }) => ({ ...signedIn, device_name: name }) });
    render(<SyncTab />);
    const field = await screen.findByLabelText("この Mac の名前");
    await userEvent.clear(field);
    await userEvent.type(field, "仕事用");
    expect(ipc.count("sync_set_device_name")).toBe(0);
    // IME 確定の Enter (keyCode 229) は保存しない
    await act(async () => { field.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", keyCode: 229, bubbles: true })); });
    expect(ipc.count("sync_set_device_name")).toBe(0);
    await userEvent.type(field, "{enter}");
    expect(ipc.last("sync_set_device_name")).toEqual({ name: "仕事用" });
    await waitFor(() => expect(field).toHaveValue("仕事用"));

    await userEvent.clear(field);
    await userEvent.type(field, "自宅");
    await userEvent.tab();
    await waitFor(() => expect(ipc.last("sync_set_device_name")).toEqual({ name: "自宅" }));
    expect(ipc.count("sync_set_device_name")).toBe(2);
  });

  it("runs sync_now and disables the button while syncing", async () => {
    const now = deferred<void>();
    let current: SyncStatus = signedIn;
    const ipc = setupIpc({ sync_status: () => current, sync_now: () => now.promise });
    render(<SyncTab />);
    await userEvent.click(await screen.findByRole("button", { name: "今すぐ同期" }));
    expect(ipc.count("sync_now")).toBe(1);
    expect(screen.getByRole("button", { name: "今すぐ同期" })).toBeDisabled();
    current = { ...signedIn, syncing: true };
    await act(async () => now.resolve());
    await emitEvent(EVENTS.sync);
    await waitFor(() => expect(screen.getByRole("button", { name: "今すぐ同期" })).toBeDisabled());
    current = signedIn;
    await emitEvent(EVENTS.sync);
    await waitFor(() => expect(screen.getByRole("button", { name: "今すぐ同期" })).toBeEnabled());
  });

  it("disables sync now when sync is off or signed out", async () => {
    setupIpc({ sync_status: () => ({ ...signedIn, enabled: false }) });
    render(<SyncTab />);
    expect(await screen.findByRole("button", { name: "今すぐ同期" })).toBeDisabled();
  });

  it("shows the sync error in one red line", async () => {
    setupIpc({ sync_status: () => ({ ...signedIn, error: "Google の再ログインが必要です" }) });
    render(<SyncTab />);
    expect(await screen.findByRole("alert")).toHaveTextContent("同期できませんでした: Google の再ログインが必要です");
  });

  it("does not double the prefix when the message already has it", async () => {
    setupIpc({ sync_status: () => ({ ...signedIn, error: "同期できませんでした: ネットワーク" }) });
    render(<SyncTab />);
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe("同期できませんでした: ネットワーク");
  });

  it("does not prefix action errors", async () => {
    setupIpc({ sync_status: () => out, sync_login: () => { throw { message: "タイムアウトしました" }; } });
    render(<SyncTab />);
    await userEvent.click(await screen.findByRole("button", { name: "Google でログイン" }));
    expect((await screen.findByRole("alert")).textContent).toBe("タイムアウトしました");
  });

  it("ignores a stale sync_status response that resolves after a newer one", async () => {
    const first = deferred();
    let calls = 0;
    setupIpc({ sync_status: () => (++calls === 1 ? first.promise : { ...signedIn, account_email: "new@gmail.com" }) });
    render(<SyncTab />);
    await waitFor(() => expect(calls).toBe(1));
    await emitEvent(EVENTS.sync);
    expect(await screen.findByText("new@gmail.com でログイン中")).toBeInTheDocument();
    await act(async () => first.resolve({ ...signedIn, account_email: "old@gmail.com" }));
    expect(screen.getByText("new@gmail.com でログイン中")).toBeInTheDocument();
    expect(screen.queryByText("old@gmail.com でログイン中")).not.toBeInTheDocument();
  });

  it("ignores a stale load that resolves after a command result", async () => {
    const first = deferred();
    let calls = 0;
    setupIpc({
      sync_status: () => (++calls === 1 ? { ...signedIn, account_email: "a@gmail.com" } : first.promise),
      sync_set_enabled: ({ enabled }) => ({ ...signedIn, account_email: "a@gmail.com", enabled }),
    });
    render(<SyncTab />);
    await screen.findByText("a@gmail.com でログイン中");
    await emitEvent(EVENTS.sync);
    await waitFor(() => expect(calls).toBe(2));
    await userEvent.click(screen.getByRole("switch", { name: "同期する" }));
    await waitFor(() => expect(screen.getByRole("switch", { name: "同期する" })).toHaveAttribute("aria-checked", "false"));
    await act(async () => first.resolve({ ...signedIn, account_email: "a@gmail.com", enabled: true }));
    expect(screen.getByRole("switch", { name: "同期する" })).toHaveAttribute("aria-checked", "false");
  });

  it("disables sync now when signed out", async () => {
    setupIpc({ sync_status: () => out });
    render(<SyncTab />);
    expect(await screen.findByRole("button", { name: "今すぐ同期" })).toBeDisabled();
  });

  it("shows 同期中… while syncing", async () => {
    setupIpc({ sync_status: () => ({ ...signedIn, syncing: true }) });
    render(<SyncTab />);
    expect(await screen.findByTestId("last-sync")).toHaveTextContent("同期中…");
    expect(screen.getByRole("button", { name: "今すぐ同期" })).toBeDisabled();
  });

  it("shows an error when sync_set_enabled fails", async () => {
    setupIpc({ sync_status: () => signedIn, sync_set_enabled: () => { throw { message: "保存できません" }; } });
    render(<SyncTab />);
    await userEvent.click(await screen.findByRole("switch", { name: "同期する" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("保存できません");
  });

  it("shows an error when sync_logout fails", async () => {
    setupIpc({ sync_status: () => signedIn, sync_logout: () => { throw { message: "ログアウトできません" }; } });
    render(<SyncTab />);
    await userEvent.click(await screen.findByRole("button", { name: "ログアウト" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("ログアウトできません");
    expect(screen.getByRole("button", { name: "ログアウト" })).toBeEnabled();
  });

  it("clears an action error after a successful refetch", async () => {
    setupIpc({ sync_status: () => signedIn, sync_logout: () => { throw { message: "ログアウトできません" }; } });
    render(<SyncTab />);
    await userEvent.click(await screen.findByRole("button", { name: "ログアウト" }));
    await screen.findByRole("alert");
    await emitEvent(EVENTS.sync);
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
  });

  it("shows a load failure", async () => {
    setupIpc({ sync_status: () => { throw { message: "boom" }; } });
    render(<SyncTab />);
    expect(await screen.findByRole("alert")).toHaveTextContent("boom");
  });

  it("refetches on sync-changed", async () => {
    let current: SyncStatus = signedIn;
    const ipc = setupIpc({ sync_status: () => current });
    render(<SyncTab />);
    await screen.findByText("you@gmail.com でログイン中");
    current = { ...signedIn, error: "失敗" };
    await emitEvent(EVENTS.sync);
    expect(await screen.findByRole("alert")).toHaveTextContent("同期できませんでした: 失敗");
    expect(ipc.count("sync_status")).toBe(2);
    current = { ...signedIn, error: null, account_email: "other@gmail.com" };
    await emitEvent(EVENTS.sync);
    expect(await screen.findByText("other@gmail.com でログイン中")).toBeInTheDocument();
    expect(screen.queryByText("you@gmail.com でログイン中")).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("uses the Drive layout for any backend that is not nas, and the new period heading", async () => {
    setupIpc({ sync_status: () => ({ ...out, backend: "google" }) });
    render(<SyncTab />);
    expect(await screen.findByRole("button", { name: "Google でログイン" })).toBeInTheDocument();
    expect(within(screen.getByRole("group", { name: "保存先" })).getByRole("button", { name: "Google ドライブ" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("heading", { name: "保存先に置く期間" })).toBeInTheDocument();
    expect(screen.queryByLabelText("NAS の URL")).not.toBeInTheDocument();
  });

  it("switches the backend and opens the guide the first time NAS is chosen with an empty URL", async () => {
    const ipc = setupIpc({ sync_status: () => out, sync_set_backend: ({ backend }) => ({ ...out, backend }) });
    render(<SyncTab />);
    await userEvent.click(within(await screen.findByRole("group", { name: "保存先" })).getByRole("button", { name: "NAS (WebDAV)" }));
    expect(ipc.last("sync_set_backend")).toEqual({ backend: "nas" });
    expect(await screen.findByRole("dialog", { name: "NAS の準備" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "閉じる" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByLabelText("NAS の URL")).toHaveValue("");
  });

  it("does not open the guide when the URL is already set", async () => {
    setupIpc({ sync_status: () => ({ ...nas, backend: "gdrive" }), sync_set_backend: ({ backend }) => ({ ...nas, backend }) });
    render(<SyncTab />);
    await userEvent.click(within(await screen.findByRole("group", { name: "保存先" })).getByRole("button", { name: "NAS (WebDAV)" }));
    expect(await screen.findByLabelText("NAS の URL")).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("switches back to Google Drive", async () => {
    const ipc = setupIpc({ sync_status: () => nas, sync_set_backend: ({ backend }) => ({ ...nas, backend }) });
    render(<SyncTab />);
    await userEvent.click(within(await screen.findByRole("group", { name: "保存先" })).getByRole("button", { name: "Google ドライブ" }));
    expect(ipc.last("sync_set_backend")).toEqual({ backend: "gdrive" });
    expect(await screen.findByRole("button", { name: "Google でログイン" })).toBeInTheDocument();
  });

  it("shows the NAS fields with a saved password mask", async () => {
    setupIpc({ sync_status: () => nas });
    render(<SyncTab />);
    expect(await screen.findByLabelText("NAS の URL")).toHaveValue("http://takeuchi-nas:5005/infodesk/");
    expect(screen.getByLabelText("ユーザー名")).toHaveValue("infodesk");
    expect(screen.getByLabelText("パスワード")).toHaveValue("••••••••");
    expect(screen.getByText("接続済み · infodesk-mac")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "ログアウト" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Google でログイン" })).not.toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "同期する" })).toBeEnabled();
  });

  it.each([
    ["needs_login", null, null, "ログインしてください", "ログインページを開く"],
    ["needs_approval", null, null, "Tailscale の管理画面でこの端末を承認してください", "Tailscale にログイン"],
    ["starting", null, null, "起動中…", "Tailscale にログイン"],
    ["failed", null, "no route", "起動できませんでした: no route", "Tailscale にログイン"],
    ["stopped", null, null, "停止中", "Tailscale にログイン"],
  ] as const)("shows the tailnet state %s", async (state, name, reason, text, button) => {
    setupIpc({ sync_status: () => ({ ...nas, tailnet: { state, name, reason } }) });
    render(<SyncTab />);
    expect(await screen.findByText(text)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: button })).toBeInTheDocument();
  });

  describe("while tailscale starts", () => {
    const start = new Date("2026-10-15T01:00:00Z");
    const starting = (extra: Partial<SyncStatus["tailnet"]> = {}): SyncStatus => ({
      ...nas,
      tailnet: { state: "starting", name: null, reason: null, started_at: start.toISOString(), phase: "Tailscale のサーバーにつないでいます", last_error: null, login_url: null, ...extra },
    });
    const at = (secs: number) => new Date(start.getTime() + secs * 1000);
    const tick = (ms: number) => act(async () => { await vi.advanceTimersByTimeAsync(ms); });

    it("shows the phase with the elapsed seconds, updated every second", async () => {
      vi.useFakeTimers({ toFake: ["Date", "setInterval", "clearInterval"], now: at(12) });
      try {
        setupIpc({ sync_status: () => starting() });
        render(<SyncTab />);
        expect(await screen.findByText("Tailscale のサーバーにつないでいます (12 秒)")).toBeInTheDocument();
        await tick(1000);
        expect(screen.getByText("Tailscale のサーバーにつないでいます (13 秒)")).toBeInTheDocument();
        await tick(2000);
        expect(screen.getByText("Tailscale のサーバーにつないでいます (15 秒)")).toBeInTheDocument();
        expect(screen.queryByText("時間がかかっています")).not.toBeInTheDocument();
      } finally {
        vi.useRealTimers();
      }
    });

    it("shows the login-preparing phase", async () => {
      vi.useFakeTimers({ toFake: ["Date", "setInterval", "clearInterval"], now: at(3) });
      try {
        setupIpc({ sync_status: () => starting({ phase: "ログインの準備をしています" }) });
        render(<SyncTab />);
        expect(await screen.findByText("ログインの準備をしています (3 秒)")).toBeInTheDocument();
      } finally {
        vi.useRealTimers();
      }
    });

    it("offers retry and reset after 30 seconds, with the last error", async () => {
      vi.useFakeTimers({ toFake: ["Date", "setInterval", "clearInterval"], now: at(28) });
      try {
        const ipc = setupIpc({
          sync_status: () => starting({ last_error: "Tailscale に問い合わせられませんでした: timed out" }),
          tailnet_restart: () => starting(),
          tailnet_forget: () => ({ ...nas, tailnet: { state: "stopped" as const, name: null, reason: null } }),
        });
        render(<SyncTab />);
        await screen.findByText("Tailscale のサーバーにつないでいます (28 秒)");
        expect(screen.queryByText("時間がかかっています")).not.toBeInTheDocument();
        await tick(2000);
        expect(screen.getByText("時間がかかっています")).toBeInTheDocument();
        expect(screen.getByText("Tailscale に問い合わせられませんでした: timed out")).toBeInTheDocument();
        await userEvent.click(screen.getByRole("button", { name: "もう一度試す" }));
        await waitFor(() => expect(ipc.count("tailnet_restart")).toBe(1));
        // 状態を消すのは確認してから。
        await userEvent.click(screen.getByRole("button", { name: "状態を消してやり直す" }));
        expect(ipc.count("tailnet_forget")).toBe(0);
        expect(screen.getByText("Tailscale にもう一度ログインが必要になります。消しますか?")).toBeInTheDocument();
        await userEvent.click(screen.getByRole("button", { name: "消す" }));
        await waitFor(() => expect(ipc.count("tailnet_forget")).toBe(1));
        await waitFor(() => expect(screen.getByText("停止中")).toBeInTheDocument());
        expect(screen.queryByText("時間がかかっています")).not.toBeInTheDocument();
      } finally {
        vi.useRealTimers();
      }
    });

    it("shows no hint before 30 seconds even with an error", async () => {
      vi.useFakeTimers({ toFake: ["Date", "setInterval", "clearInterval"], now: at(5) });
      try {
        setupIpc({ sync_status: () => starting({ last_error: "boom" }) });
        render(<SyncTab />);
        await screen.findByText("Tailscale のサーバーにつないでいます (5 秒)");
        expect(screen.queryByText("時間がかかっています")).not.toBeInTheDocument();
        expect(screen.queryByRole("button", { name: "もう一度試す" })).not.toBeInTheDocument();
      } finally {
        vi.useRealTimers();
      }
    });
  });

  it("opens the login page again when tailscale needs a login", async () => {
    const waiting = { ...nas, tailnet: { state: "needs_login" as const, name: null, reason: null, login_url: "https://login.tailscale.com/a/xyz" } };
    const ipc = setupIpc({ sync_status: () => waiting, tailnet_login: () => waiting });
    render(<SyncTab />);
    const open = await screen.findByRole("button", { name: "ログインページを開く" });
    expect(open).toHaveClass("btn-primary");
    await userEvent.click(open);
    expect(ipc.count("tailnet_login")).toBe(1);
    expect(screen.queryByRole("button", { name: "Tailscale にログイン" })).not.toBeInTheDocument();
  });

  it("logs in to and out of tailscale", async () => {
    const loggedOut = { ...nas, tailnet: { state: "needs_login" as const, name: null, reason: null } };
    const ipc = setupIpc({ sync_status: () => loggedOut, tailnet_login: () => nas, tailnet_logout: () => loggedOut });
    render(<SyncTab />);
    await userEvent.click(await screen.findByRole("button", { name: "ログインページを開く" }));
    expect(ipc.count("tailnet_login")).toBe(1);
    await userEvent.click(await screen.findByRole("button", { name: "ログアウト" }));
    expect(ipc.count("tailnet_logout")).toBe(1);
    expect(await screen.findByRole("button", { name: "ログインページを開く" })).toBeInTheDocument();
  });

  it("saves the URL and user with a null password", async () => {
    const ipc = setupIpc({ sync_status: () => nas, sync_set_nas: ({ url, user }) => ({ ...nas, nas_url: url, nas_user: user }) });
    render(<SyncTab />);
    const url = await screen.findByLabelText("NAS の URL");
    await userEvent.clear(url);
    await userEvent.type(url, "http://x:5005/d/{enter}");
    expect(ipc.last("sync_set_nas")).toEqual({ url: "http://x:5005/d/", user: "infodesk", password: null });
    const user = screen.getByLabelText("ユーザー名");
    await userEvent.clear(user);
    await userEvent.type(user, "bob");
    await userEvent.tab();
    await waitFor(() => expect(ipc.last("sync_set_nas")).toEqual({ url: "http://x:5005/d/", user: "bob", password: null }));
  });

  it("sends the password only when typed", async () => {
    const ipc = setupIpc({ sync_status: () => nas, sync_set_nas: () => nas });
    render(<SyncTab />);
    const pw = await screen.findByLabelText("パスワード");
    await userEvent.click(pw);
    await userEvent.tab();
    expect(ipc.count("sync_set_nas")).toBe(0);
    expect(pw).toHaveValue("••••••••");
    await userEvent.click(pw);
    await userEvent.type(pw, "s3cret{enter}");
    await waitFor(() => expect(ipc.last("sync_set_nas")).toEqual({ url: nas.nas_url, user: "infodesk", password: "s3cret" }));
    await waitFor(() => expect(pw).toHaveValue("••••••••"));
  });

  it("tests the connection and shows green or red", async () => {
    let result = "読み書きできました";
    const ipc = setupIpc({ sync_status: () => nas, sync_test_nas: () => result });
    render(<SyncTab />);
    await userEvent.click(await screen.findByRole("button", { name: "確かめる" }));
    const ok = await screen.findByTestId("nas-test-result");
    expect(ok).toHaveTextContent("読み書きできました");
    expect(ok).toHaveClass("success");
    result = "認証できません";
    await userEvent.click(screen.getByRole("button", { name: "確かめる" }));
    await waitFor(() => expect(screen.getByTestId("nas-test-result")).toHaveTextContent("認証できません"));
    expect(screen.getByTestId("nas-test-result")).toHaveClass("error");
    expect(ipc.count("sync_test_nas")).toBe(2);
  });

  it("opens the guide from the link", async () => {
    setupIpc({ sync_status: () => nas });
    render(<SyncTab />);
    await userEvent.click(await screen.findByRole("button", { name: "NAS の準備を見る (初めての方へ)" }));
    expect(screen.getByRole("dialog", { name: "NAS の準備" })).toBeInTheDocument();
  });

  it("disables sync now until tailscale is connected", async () => {
    setupIpc({ sync_status: () => ({ ...nas, tailnet: { state: "needs_login", name: null, reason: null } }) });
    render(<SyncTab />);
    expect(await screen.findByRole("button", { name: "今すぐ同期" })).toBeDisabled();
  });

  it("lets you turn sync off when tailscale dropped while it was enabled", async () => {
    const dropped = { ...nas, enabled: true, tailnet: { state: "needs_login" as const, name: null, reason: null } };
    const ipc = setupIpc({ sync_status: () => dropped, sync_set_enabled: ({ enabled }) => ({ ...dropped, enabled }) });
    render(<SyncTab />);
    const toggle = await screen.findByRole("switch", { name: "同期する" });
    expect(toggle).toBeEnabled();
    await userEvent.click(toggle);
    expect(ipc.last("sync_set_enabled")).toEqual({ enabled: false });
    await waitFor(() => expect(screen.getByRole("switch", { name: "同期する" })).toBeDisabled());
  });

  it("clears the connection result when the URL is saved", async () => {
    setupIpc({ sync_status: () => nas, sync_test_nas: () => "読み書きできました", sync_set_nas: ({ url }) => ({ ...nas, nas_url: url }) });
    render(<SyncTab />);
    await userEvent.click(await screen.findByRole("button", { name: "確かめる" }));
    await screen.findByTestId("nas-test-result");
    const url = screen.getByLabelText("NAS の URL");
    await userEvent.clear(url);
    await userEvent.type(url, "http://x:5005/d/{enter}");
    await waitFor(() => expect(screen.queryByTestId("nas-test-result")).not.toBeInTheDocument());
  });

  it("clears the connection result when the backend changes", async () => {
    setupIpc({ sync_status: () => nas, sync_test_nas: () => "認証できません", sync_set_backend: ({ backend }) => ({ ...nas, backend }) });
    render(<SyncTab />);
    await userEvent.click(await screen.findByRole("button", { name: "確かめる" }));
    await screen.findByTestId("nas-test-result");
    await userEvent.click(within(screen.getByRole("group", { name: "保存先" })).getByRole("button", { name: "Google ドライブ" }));
    await userEvent.click(await within(screen.getByRole("group", { name: "保存先" })).findByRole("button", { name: "NAS (WebDAV)" }));
    expect(screen.queryByTestId("nas-test-result")).not.toBeInTheDocument();
  });

  it("shows a check icon on success and an alert icon on failure", async () => {
    let result = "読み書きできました";
    setupIpc({ sync_status: () => nas, sync_test_nas: () => result });
    render(<SyncTab />);
    await userEvent.click(await screen.findByRole("button", { name: "確かめる" }));
    expect((await screen.findByTestId("nas-test-result")).querySelector("svg.lucide-circle-check")).not.toBeNull();
    result = "認証できません";
    await userEvent.click(screen.getByRole("button", { name: "確かめる" }));
    await waitFor(() => expect(screen.getByTestId("nas-test-result").querySelector("svg.lucide-circle-alert")).not.toBeNull());
  });

  it("shows busy labels while the connection test and tailnet logout run", async () => {
    const test = deferred<string>();
    const logout = deferred();
    setupIpc({ sync_status: () => nas, sync_test_nas: () => test.promise, tailnet_logout: () => logout.promise });
    render(<SyncTab />);
    await userEvent.click(await screen.findByRole("button", { name: "確かめる" }));
    expect(screen.getByRole("button", { name: "確かめています…" })).toBeDisabled();
    await act(async () => test.resolve("読み書きできました"));
    expect(await screen.findByRole("button", { name: "確かめる" })).toBeEnabled();
    await userEvent.click(screen.getByRole("button", { name: "ログアウト" }));
    expect(screen.getByRole("button", { name: "ログアウト中…" })).toBeDisabled();
    await act(async () => logout.resolve(nas));
    expect(await screen.findByRole("button", { name: "ログアウト" })).toBeEnabled();
  });

  it("forgets the local tailscale state after a confirm when it failed", async () => {
    const failed = { ...nas, tailnet: { state: "failed" as const, name: null, reason: "boom" } };
    const ipc = setupIpc({ sync_status: () => failed, tailnet_forget: () => ({ ...nas, tailnet: { state: "needs_login" as const, name: null, reason: null } }) });
    render(<SyncTab />);
    await userEvent.click(await screen.findByRole("button", { name: "この Mac の Tailscale の状態を消す" }));
    expect(ipc.count("tailnet_forget")).toBe(0);
    expect(screen.getByText("Tailscale にもう一度ログインが必要になります。消しますか?")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "やめる" }));
    expect(screen.queryByRole("button", { name: "消す" })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "この Mac の Tailscale の状態を消す" }));
    await userEvent.click(screen.getByRole("button", { name: "消す" }));
    expect(ipc.count("tailnet_forget")).toBe(1);
    expect(await screen.findByText("ログインしてください")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "この Mac の Tailscale の状態を消す" })).not.toBeInTheDocument();
  });

  it("saves the user and password while the URL is still empty", async () => {
    const empty = { ...nas, nas_url: "", nas_user: "", nas_password_set: false };
    const ipc = setupIpc({ sync_status: () => empty, sync_set_nas: ({ url, user, password }) => ({ ...empty, nas_url: url, nas_user: user, nas_password_set: password !== null }) });
    render(<SyncTab />);
    const user = await screen.findByLabelText("ユーザー名");
    await userEvent.type(user, "bob{enter}");
    await waitFor(() => expect(ipc.last("sync_set_nas")).toEqual({ url: "", user: "bob", password: null }));
    await userEvent.click(screen.getByLabelText("パスワード"));
    await userEvent.type(screen.getByLabelText("パスワード"), "pw{enter}");
    await waitFor(() => expect(ipc.last("sync_set_nas")).toEqual({ url: "", user: "bob", password: "pw" }));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("clears the saved NAS settings after an inline confirm", async () => {
    const cleared = { ...nas, nas_url: "", nas_user: "", nas_password_set: false };
    const ipc = setupIpc({ sync_status: () => nas, sync_clear_nas: () => cleared });
    render(<SyncTab />);
    await userEvent.click(await screen.findByRole("button", { name: "NAS の設定を消す" }));
    expect(ipc.count("sync_clear_nas")).toBe(0);
    expect(screen.getByText("NAS の URL、ユーザー名、パスワードを消します。消しますか?")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "やめる" }));
    expect(ipc.count("sync_clear_nas")).toBe(0);
    await userEvent.click(screen.getByRole("button", { name: "NAS の設定を消す" }));
    await userEvent.click(screen.getByRole("button", { name: "消す" }));
    expect(ipc.count("sync_clear_nas")).toBe(1);
    await waitFor(() => expect(screen.getByLabelText("NAS の URL")).toHaveValue(""));
    expect(screen.getByLabelText("ユーザー名")).toHaveValue("");
    expect(screen.getByLabelText("パスワード")).toHaveValue("");
    // 消す物がなければ出さない。
    expect(screen.queryByRole("button", { name: "NAS の設定を消す" })).not.toBeInTheDocument();
  });

  it("does not offer forget unless the tailnet failed", async () => {
    setupIpc({ sync_status: () => nas });
    render(<SyncTab />);
    await screen.findByLabelText("NAS の URL");
    expect(screen.queryByRole("button", { name: "この Mac の Tailscale の状態を消す" })).not.toBeInTheDocument();
  });

  it("suppresses global shortcuts while the guide is open", async () => {
    const collect = vi.fn();
    function Harness() {
      const ref = useRef<ViewActions>({});
      useShortcuts(true, { collect }, ref);
      return <SyncTab />;
    }
    setupIpc({ sync_status: () => nas });
    render(<Harness />);
    await screen.findByLabelText("NAS の URL");
    await userEvent.keyboard("{Control>}r{/Control}");
    expect(collect).toHaveBeenCalledTimes(1);
    await userEvent.click(screen.getByRole("button", { name: "NAS の準備を見る (初めての方へ)" }));
    await userEvent.keyboard("{Control>}r{/Control}");
    expect(collect).toHaveBeenCalledTimes(1);
  });
});
