import { useCallback, useEffect, useRef, useState } from "react";
import { api, EVENTS } from "../../api";
import { ErrorLine } from "../../components/ErrorLine";
import { Toggle } from "../../components/Toggle";
import { useTauriEvent } from "../../hooks/useTauriEvent";
import { formatClock } from "../../lib/dates";
import { errorMessage, type SyncStatus } from "../../types";
import { SettingField, type ActionResult } from "./shared";

const SYNC_FAILED = "同期できませんでした";
const DRIVE_DESC = "収集した記事とダイジェストを、Google ドライブのアプリ専用の領域に置きます。スマホの InfoDesk から読めます。";

export function SyncTab() {
  const [status, setStatus] = useState<SyncStatus | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [busy, setBusy] = useState<ReadonlySet<string>>(new Set());
  const inFlight = useRef(new Set<string>());

  const ticket = useRef(0);

  const load = useCallback(() => {
    const t = ++ticket.current;
    return api.syncStatus()
      .then((s) => { if (t === ticket.current) { setStatus(s); setLoadError(null); setActionError(null); } })
      .catch((e) => { if (t === ticket.current) setLoadError(errorMessage(e)); });
  }, []);
  useEffect(() => { void load(); }, [load]);
  useTauriEvent(EVENTS.sync, () => { void load(); });

  /** name ごとに同時実行を1件に制限する。返り値の SyncStatus があれば表示に反映する。 */
  const run = useCallback((name: string, fn: () => Promise<SyncStatus | void>): Promise<ActionResult> => {
    if (inFlight.current.has(name)) return Promise.resolve({ ok: false, error: null });
    inFlight.current.add(name);
    setBusy(new Set(inFlight.current));
    return fn()
      .then((s) => { if (s) { ticket.current++; setStatus(s); } setActionError(null); return { ok: true, error: null }; })
      .catch((e) => { const error = errorMessage(e); setActionError(error); return { ok: false, error }; })
      .finally(() => { inFlight.current.delete(name); setBusy(new Set(inFlight.current)); });
  }, []);

  if (!status) return <ErrorLine message={loadError} />;
  const canSync = status.signed_in;
  return (
    <>
      <ErrorLine message={loadError} />
      <ErrorLine message={actionError} />
      <section className="section">
        <h2>Google ドライブ</h2>
        <p className="desc">{DRIVE_DESC}</p>
        {!status.available ? (
          <p className="desc">このビルドでは同期を使えません</p>
        ) : (
          <div className="setting-row">
            <div className="label">
              <span>Google アカウント</span>
              <span className="hint">
                {status.signed_in
                  ? `${status.account_email ?? ""} でログイン中`
                  : "ログインすると、ブラウザで Google の画面が開きます。ID やパスワードはアプリに入力しません。"}
              </span>
            </div>
            {status.signed_in ? (
              <button className="btn" disabled={busy.has("logout")} onClick={() => run("logout", () => api.syncLogout())}>ログアウト</button>
            ) : (
              <button className="btn btn-primary" disabled={busy.has("login")} onClick={() => run("login", () => api.syncLogin())}>
                {busy.has("login") ? "ブラウザでログインしてください…" : "Google でログイン"}
              </button>
            )}
          </div>
        )}
      </section>
      {status.available && (
        <>
          <section className="section">
            <h2>同期</h2>
            <div className="setting-row">
              <div className="label"><span>同期する</span><span className="hint">収集のあと、ダイジェストを作ったあと、⌘R のときに同期します</span></div>
              <Toggle checked={status.enabled} ariaLabel="同期する" disabled={!canSync || busy.has("enabled")} onChange={(v) => run("enabled", () => api.syncSetEnabled(v))} />
            </div>
            <SettingField id="device-name" label="この Mac の名前" hint="スマホの同期の記録に出ます" value={status.device_name}
              onSave={(v) => run("name", () => api.syncSetDeviceName(v))} />
            <div className="setting-row">
              <div className="label"><span>最終同期</span><span className="hint">スマホで付けた既読とスターも、このときに取り込みます</span></div>
              <span className="small text-secondary" data-testid="last-sync">{status.syncing ? "同期中…" : status.last_sync_at ? formatClock(status.last_sync_at) : "—"}</span>
              <button className="btn" disabled={!canSync || !status.enabled || status.syncing || busy.has("now")} onClick={() => run("now", () => api.syncNow())}>今すぐ同期</button>
            </div>
            <ErrorLine message={status.error && (status.error.startsWith(SYNC_FAILED) ? status.error : `${SYNC_FAILED}: ${status.error}`)} />
          </section>
          <section className="section">
            <h2>Drive に置く期間</h2>
            <p className="desc">記事と日次ダイジェストは直近60日、週次と月次のダイジェストは直近400日。スマホの控えは消えません。</p>
          </section>
        </>
      )}
    </>
  );
}
