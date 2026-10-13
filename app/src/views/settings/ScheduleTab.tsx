import { api } from "../../api";
import { ErrorLine } from "../../components/ErrorLine";
import { Toggle } from "../../components/Toggle";
import { formatClock, localDateString } from "../../lib/dates";
import type { AppStatus } from "../../types";
import { SettingField, useSettingsState, withStoredOption } from "./shared";

const intervalLabel = (m: string) => {
  const n = Number(m);
  return n >= 60 && n % 60 === 0 ? `${n / 60}時間おき` : `${m}分おき`;
};
const INTERVALS = [
  ["1", "1分おき"], ["5", "5分おき"], ["15", "15分おき"], ["30", "30分おき"],
  ["60", "1時間おき"], ["180", "3時間おき"], ["360", "6時間おき"], ["720", "12時間おき"],
].map(([id, label]) => ({ id, label }));
const WEEKDAYS = [["mon", "月曜"], ["tue", "火曜"], ["wed", "水曜"], ["thu", "木曜"], ["fri", "金曜"], ["sat", "土曜"], ["sun", "日曜"]].map(([id, label]) => ({ id, label }));

/** 次回の収集時刻。今日 (ローカル日付) なら HH:MM、別の日なら M/D HH:MM。 */
function nextCollectLabel(status: AppStatus, now: Date): string {
  if (status.collecting) return "収集中…";
  if (!status.next_collect_at) return "—";
  const at = new Date(status.next_collect_at);
  const clock = formatClock(status.next_collect_at);
  return localDateString(at) === localDateString(now) ? clock : `${at.getMonth() + 1}/${at.getDate()} ${clock}`;
}

const TIME_RE = /^(\d{1,2}):(\d{2})$/;
const TIME_ERROR = "HH:MM の形式で入力してください";
const validateTime = (v: string) => {
  const m = TIME_RE.exec(v.trim());
  return m && Number(m[1]) <= 23 && Number(m[2]) <= 59 ? null : TIME_ERROR;
};
const normalizeTime = (v: string) => {
  const m = TIME_RE.exec(v.trim());
  return m ? `${m[1].padStart(2, "0")}:${m[2]}` : v;
};

export function ScheduleTab({ status }: { status: AppStatus }) {
  const { settings, loadError, actionError, busy, act, save } = useSettingsState();
  if (!settings) return <ErrorLine message={loadError} />;
  const g = (k: string) => settings[k] ?? "";
  // 分の設定が未設定なら、旧設定 (時間) を分に直して表示する。
  const hours = Number(g("schedule.collect_interval_hours"));
  const interval = g("schedule.collect_interval_minutes") || (Number.isInteger(hours) && hours > 0 ? String(hours * 60) : "");
  const timeField = (id: string, label: string, key: string, hint?: string) => (
    <SettingField id={id} label={label} width={120} hint={hint} value={g(key)} validate={validateTime} normalize={normalizeTime} onSave={(v) => save(key, v)} />
  );
  return (
    <>
      <ErrorLine message={loadError} />
      <ErrorLine message={actionError} />
      <section className="section">
        <h2>収集</h2>
        <div className="setting-row">
          <div className="label"><label htmlFor="interval">収集間隔</label></div>
          <select id="interval" className="field" style={{ width: 180 }} value={interval} onChange={(e) => save("schedule.collect_interval_minutes", e.target.value)}>
            {withStoredOption(INTERVALS, interval, intervalLabel).map((o) => <option key={o.id} value={o.id}>{o.label}</option>)}
          </select>
        </div>
        <div className="setting-row">
          <div className="label"><span>次回の収集</span><span className="hint">手動で今すぐ収集もできます</span></div>
          <span className="small text-secondary" data-testid="next-collect">{nextCollectLabel(status, new Date())}</span>
          <button className="btn" disabled={status.collecting || busy.has("collect")} onClick={() => act("collect", () => api.collectNow())}>今すぐ収集</button>
        </div>
      </section>
      <section className="section">
        <h2>要約の生成</h2>
        <p className="desc">日次の時刻に前日の日次要約を確定し、現在の週次と月次を作り直します。収集では要約を作り直しません。</p>
        {timeField("daily-time", "日次要約の時刻", "schedule.daily_time", "HH:MM")}
        <div className="setting-row">
          <div className="label"><label htmlFor="weekly-day">週次要約の曜日</label><span className="hint">日次要約から生成</span></div>
          <select id="weekly-day" className="field" style={{ width: 120 }} value={g("schedule.weekly_day")} onChange={(e) => save("schedule.weekly_day", e.target.value)}>
            {withStoredOption(WEEKDAYS, g("schedule.weekly_day")).map((o) => <option key={o.id} value={o.id}>{o.label}</option>)}
          </select>
        </div>
        {timeField("weekly-time", "週次要約の時刻", "schedule.weekly_time")}
        {timeField("monthly-time", "月次要約の時刻 (毎月1日)", "schedule.monthly_time", "HH:MM。週次要約から生成")}
      </section>
      <section className="section">
        <h2>起動</h2>
        <div className="setting-row">
          <div className="label"><span>ログイン時に起動</span><span className="hint">メニューバーに常駐し、ウィンドウは開きません</span></div>
          <Toggle checked={g("app.launch_at_login") === "true"} ariaLabel="ログイン時に起動" disabled={busy.has("login")} onChange={(v) => act("login", () => api.setLaunchAtLogin(v))} />
        </div>
        <div className="setting-row">
          <div className="label"><span>ウィンドウを閉じても常駐する</span><span className="hint">オフにすると閉じた時に終了します</span></div>
          <Toggle checked={g("app.stay_resident") !== "false"} ariaLabel="ウィンドウを閉じても常駐する" disabled={busy.has("set:app.stay_resident")} onChange={(v) => save("app.stay_resident", v ? "true" : "false")} />
        </div>
      </section>
      <section className="section">
        <h2>保留中の要約</h2>
        <p className="desc">LLMに接続できず生成できなかった要約は、次回のスケジュールで再試行します。手動で今すぐ再試行することもできます (失敗した要約も含みます)。</p>
        <div className="setting-row">
          <div className="label"><span>{status.last_error ? `最後のエラー: ${status.last_error}` : "エラーはありません"}</span></div>
          <button className="btn" disabled={busy.has("retry")} onClick={() => act("retry", () => api.retryPendingDigests())}>保留中の要約を再試行</button>
        </div>
      </section>
    </>
  );
}
