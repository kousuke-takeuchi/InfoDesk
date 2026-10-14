import { describe, expect, it } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SyncTab } from "./SyncTab";
import { emitEvent, setupIpc } from "../../test/ipc";
import { EVENTS } from "../../api";
import type { SyncStatus } from "../../types";

const out: SyncStatus = {
  available: true, signed_in: false, account_email: null, enabled: false, syncing: false,
  last_sync_at: null, error: null, device_name: "MacBook Pro",
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
    expect(screen.getByText("収集した記事とダイジェストを、Google ドライブのアプリ専用の領域に置きます。スマホの InfoDesk から読めます。")).toBeInTheDocument();
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
    expect(screen.getByText(/アプリ専用の領域に置きます/)).toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
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
    setupIpc({ sync_status: () => ({ ...signedIn, error: "同期できませんでした: ネットワークに接続できません。次の同期で再試行します。" }) });
    render(<SyncTab />);
    expect(await screen.findByRole("alert")).toHaveTextContent("同期できませんでした: ネットワークに接続できません。次の同期で再試行します。");
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
    expect(await screen.findByRole("alert")).toHaveTextContent("失敗");
    expect(ipc.count("sync_status")).toBe(2);
  });
});
