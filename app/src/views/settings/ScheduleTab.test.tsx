import { describe, expect, it } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ScheduleTab } from "./ScheduleTab";
import { setupIpc } from "../../test/ipc";
import type { AppStatus } from "../../types";

const settings = {
  "schedule.collect_interval_minutes": "180", "schedule.daily_time": "07:00", "schedule.weekly_day": "mon",
  "schedule.weekly_time": "07:30", "schedule.monthly_time": "08:00", "app.launch_at_login": "false", "app.stay_resident": "true",
};
const status: AppStatus = { collecting: false, summarizing: false, digesting: false, last_collect_at: null, next_collect_at: "2026-10-06T03:00:00Z", last_error: null };

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((r) => { resolve = r; });
  return { promise, resolve };
}

describe("ScheduleTab", () => {
  it("shows schedule settings and saves them", async () => {
    const ipc = setupIpc({ get_settings: () => settings, set_setting: () => undefined, set_launch_at_login: () => undefined, collect_now: () => undefined, retry_pending_digests: () => undefined });
    render(<ScheduleTab status={status} />);
    expect(await screen.findByLabelText("収集間隔")).toHaveValue("180");
    expect(screen.getByText(/次回の収集/)).toBeInTheDocument();

    expect(screen.getAllByRole("option", { name: /おき$/ }).map((o) => [o.getAttribute("value"), o.textContent])).toEqual([
      ["1", "1分おき"], ["5", "5分おき"], ["15", "15分おき"], ["30", "30分おき"],
      ["60", "1時間おき"], ["180", "3時間おき"], ["360", "6時間おき"], ["720", "12時間おき"],
    ]);
    await userEvent.selectOptions(screen.getByLabelText("収集間隔"), "5");
    expect(ipc.last("set_setting")).toEqual({ key: "schedule.collect_interval_minutes", value: "5" });

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

  it.each([["日次要約の時刻"], ["週次要約の時刻"], ["月次要約の時刻 (毎月1日)"]])("rejects a malformed time in %s without calling a command", async (label) => {
    const ipc = setupIpc({ get_settings: () => settings, set_setting: () => undefined });
    render(<ScheduleTab status={status} />);
    const field = await screen.findByLabelText(label);
    for (const bad of ["24:00", "7:60", "abc", "0700", "7:5", ""]) {
      await userEvent.clear(field);
      if (bad) await userEvent.type(field, bad);
      await userEvent.type(field, "{enter}");
      expect(await screen.findByText("HH:MM の形式で入力してください")).toBeInTheDocument();
    }
    expect(ipc.count("set_setting")).toBe(0);
    expect(field).toHaveValue("");
  });

  it("saves a valid time zero-padded", async () => {
    const ipc = setupIpc({ get_settings: () => settings, set_setting: () => undefined });
    render(<ScheduleTab status={status} />);
    const daily = await screen.findByLabelText("日次要約の時刻");
    await userEvent.clear(daily);
    await userEvent.type(daily, "7:05{enter}");
    expect(ipc.last("set_setting")).toEqual({ key: "schedule.daily_time", value: "07:05" });
  });

  it("clears the inline time error once a valid value is saved", async () => {
    setupIpc({ get_settings: () => settings, set_setting: () => undefined });
    render(<ScheduleTab status={status} />);
    const daily = await screen.findByLabelText("日次要約の時刻");
    await userEvent.clear(daily);
    await userEvent.type(daily, "99:99{enter}");
    expect(await screen.findByText("HH:MM の形式で入力してください")).toBeInTheDocument();
    await userEvent.clear(daily);
    await userEvent.type(daily, "08:15{enter}");
    await waitFor(() => expect(screen.queryByText("HH:MM の形式で入力してください")).not.toBeInTheDocument());
  });

  it("keeps the typed time and shows the backend error when saving fails", async () => {
    setupIpc({ get_settings: () => settings, set_setting: () => { throw { message: "storage failed" }; } });
    render(<ScheduleTab status={status} />);
    const daily = await screen.findByLabelText("日次要約の時刻");
    await userEvent.clear(daily);
    await userEvent.type(daily, "06:30{enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent("storage failed");
    expect(screen.getByLabelText("日次要約の時刻")).toHaveValue("06:30");
  });

  it("disables each action button while its own request is in flight", async () => {
    const collect = deferred();
    const retry = deferred();
    const login = deferred();
    const ipc = setupIpc({
      get_settings: () => settings, set_setting: () => undefined,
      collect_now: () => collect.promise, retry_pending_digests: () => retry.promise, set_launch_at_login: () => login.promise,
    });
    render(<ScheduleTab status={status} />);
    await screen.findByLabelText("収集間隔");

    await userEvent.click(screen.getByRole("button", { name: "今すぐ収集" }));
    expect(screen.getByRole("button", { name: "今すぐ収集" })).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "今すぐ収集" }));
    expect(ipc.count("collect_now")).toBe(1);
    expect(screen.getByRole("button", { name: "保留中の要約を再試行" })).toBeEnabled();
    await act(async () => collect.resolve());
    await waitFor(() => expect(screen.getByRole("button", { name: "今すぐ収集" })).toBeEnabled());

    await userEvent.click(screen.getByRole("button", { name: "保留中の要約を再試行" }));
    expect(screen.getByRole("button", { name: "保留中の要約を再試行" })).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "保留中の要約を再試行" }));
    expect(ipc.count("retry_pending_digests")).toBe(1);
    await act(async () => retry.resolve());
    await waitFor(() => expect(screen.getByRole("button", { name: "保留中の要約を再試行" })).toBeEnabled());

    await userEvent.click(screen.getByRole("switch", { name: "ログイン時に起動" }));
    expect(screen.getByRole("switch", { name: "ログイン時に起動" })).toBeDisabled();
    await userEvent.click(screen.getByRole("switch", { name: "ログイン時に起動" }));
    expect(ipc.count("set_launch_at_login")).toBe(1);
    await act(async () => login.resolve());
    await waitFor(() => expect(screen.getByRole("switch", { name: "ログイン時に起動" })).toBeEnabled());
  });

  it("disables the stay-resident toggle while its save is in flight", async () => {
    const gate = deferred();
    const ipc = setupIpc({ get_settings: () => settings, set_setting: () => gate.promise });
    render(<ScheduleTab status={status} />);
    const sw = await screen.findByRole("switch", { name: "ウィンドウを閉じても常駐する" });
    await userEvent.click(sw);
    expect(screen.getByRole("switch", { name: "ウィンドウを閉じても常駐する" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "ウィンドウを閉じても常駐する" })).toHaveAttribute("disabled");
    expect(document.querySelector("fieldset")).toBeNull();
    await userEvent.click(screen.getByRole("switch", { name: "ウィンドウを閉じても常駐する" }));
    expect(ipc.count("set_setting")).toBe(1);
    await act(async () => gate.resolve());
    await waitFor(() => expect(screen.getByRole("switch", { name: "ウィンドウを閉じても常駐する" })).toBeEnabled());
  });

  it("keeps the action error until the next successful action, which clears it on the next successful action", async () => {
    let fail = true;
    const ipc = setupIpc({
      get_settings: () => settings, set_setting: () => undefined,
      collect_now: () => { if (fail) throw { message: "collect failed" }; },
    });
    render(<ScheduleTab status={status} />);
    await screen.findByLabelText("収集間隔");
    await userEvent.click(screen.getByRole("button", { name: "今すぐ収集" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("collect failed");

    await userEvent.click(screen.getByRole("button", { name: "今すぐ収集" }));
    await waitFor(() => expect(ipc.count("collect_now")).toBe(2));
    expect(screen.getByRole("alert")).toHaveTextContent("collect failed");

    fail = false;
    await userEvent.click(screen.getByRole("button", { name: "今すぐ収集" }));
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
  });

  it("shows a stored interval outside the option list as itself and sends nothing until another is picked", async () => {
    const ipc = setupIpc({ get_settings: () => ({ ...settings, "schedule.collect_interval_minutes": "120" }), set_setting: () => undefined });
    render(<ScheduleTab status={status} />);
    const interval = await screen.findByLabelText("収集間隔");
    expect(interval).toHaveValue("120");
    expect(screen.getByRole("option", { name: "2時間おき" })).toHaveProperty("selected", true);
    expect(ipc.count("set_setting")).toBe(0);
    await userEvent.selectOptions(interval, screen.getByRole("option", { name: "1時間おき" }));
    expect(ipc.last("set_setting")).toEqual({ key: "schedule.collect_interval_minutes", value: "60" });
    expect(ipc.count("set_setting")).toBe(1);
  });

  it("labels a stored interval that is not a whole number of hours in minutes", async () => {
    setupIpc({ get_settings: () => ({ ...settings, "schedule.collect_interval_minutes": "90" }), set_setting: () => undefined });
    render(<ScheduleTab status={status} />);
    expect(await screen.findByLabelText("収集間隔")).toHaveValue("90");
    expect(screen.getByRole("option", { name: "90分おき" })).toHaveProperty("selected", true);
  });

  it("falls back to the old hours setting, converted to minutes, when minutes is unset", async () => {
    const { "schedule.collect_interval_minutes": _unused, ...rest } = settings;
    void _unused;
    setupIpc({ get_settings: () => ({ ...rest, "schedule.collect_interval_hours": "6" }), set_setting: () => undefined });
    render(<ScheduleTab status={status} />);
    expect(await screen.findByLabelText("収集間隔")).toHaveValue("360");
    expect(screen.getByRole("option", { name: "6時間おき" })).toHaveProperty("selected", true);
  });

  it("shows a stored weekday outside the option list as its raw value", async () => {
    const ipc = setupIpc({ get_settings: () => ({ ...settings, "schedule.weekly_day": "funday" }), set_setting: () => undefined });
    render(<ScheduleTab status={status} />);
    expect(await screen.findByLabelText("週次要約の曜日")).toHaveValue("funday");
    expect(screen.getByRole("option", { name: "funday" })).toHaveProperty("selected", true);
    expect(ipc.count("set_setting")).toBe(0);
  });

  it("shows the next collection as a time today, with the date on another day, and as running while collecting", async () => {
    setupIpc({ get_settings: () => settings });
    const pad = (n: number) => String(n).padStart(2, "0");
    const clock = (d: Date) => `${pad(d.getHours())}:${pad(d.getMinutes())}`;
    const now = new Date();
    const later = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1, 4, 5);
    const view = render(<ScheduleTab status={{ ...status, next_collect_at: now.toISOString() }} />);
    await screen.findByLabelText("収集間隔");
    expect(screen.getByTestId("next-collect")).toHaveTextContent(new RegExp(`^${clock(now)}$`));

    view.rerender(<ScheduleTab status={{ ...status, next_collect_at: later.toISOString() }} />);
    expect(screen.getByTestId("next-collect")).toHaveTextContent(new RegExp(`^${later.getMonth() + 1}/${later.getDate()} 04:05$`));

    view.rerender(<ScheduleTab status={{ ...status, collecting: true, next_collect_at: later.toISOString() }} />);
    expect(screen.getByTestId("next-collect")).toHaveTextContent(/^収集中…$/);

    view.rerender(<ScheduleTab status={{ ...status, next_collect_at: null }} />);
    expect(screen.getByTestId("next-collect")).toHaveTextContent(/^—$/);
  });

  it("shows a load error from get_settings", async () => {
    setupIpc({ get_settings: () => { throw { message: "db locked" }; } });
    render(<ScheduleTab status={status} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("db locked");
  });
});
