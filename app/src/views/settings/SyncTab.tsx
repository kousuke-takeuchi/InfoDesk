import { BookOpen, CircleAlert, CircleCheck } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { api, EVENTS } from "../../api";
import { ErrorLine } from "../../components/ErrorLine";
import { Segmented } from "../../components/Segmented";
import { Toggle } from "../../components/Toggle";
import { useTauriEvent } from "../../hooks/useTauriEvent";
import { formatClock } from "../../lib/dates";
import { errorMessage, type SyncStatus, type TailnetStatus } from "../../types";
import { NasGuideDialog } from "./NasGuideDialog";
import { isImeEnter, SettingField, type ActionResult } from "./shared";

const SYNC_FAILED = "同期できませんでした";
const STORAGE_DESC = "収集した記事とダイジェストを置く場所。スマホの InfoDesk も同じ場所から読めます。";
/** Rust の `sync::NAS_CHECK_OK` と同じ文字列。成功の判定はこの文字列との一致に頼っているので、変えるときは両方そろえる。 */
const NAS_CHECK_OK = "読み書きできました";
const MASK = "••••••••";

function tailnetText(t: TailnetStatus): string {
  switch (t.state) {
    case "running": return `接続済み · ${t.name ?? ""}`;
    case "needs_login": return "ログインしてください";
    case "needs_approval": return "Tailscale の管理画面でこの端末を承認してください";
    case "starting": return "起動中…";
    case "failed": return `起動できませんでした: ${t.reason ?? ""}`;
    default: return "停止中";
  }
}

/** 保存済みなら伏せ字を見せ、入力したときだけ送るパスワード欄。 */
function PasswordField({ isSet, onSave }: { isSet: boolean; onSave: (pw: string) => Promise<ActionResult> }) {
  const [draft, setDraft] = useState<string | null>(null); // null は未入力 (保存済みの伏せ字を表示)
  const commit = () => {
    if (draft === null) return;
    if (draft === "") { setDraft(null); return; }
    void onSave(draft).then((r) => { if (r.ok) setDraft(null); });
  };
  return (
    <div className="setting-row">
      <div className="label"><label htmlFor="nas-password">パスワード</label><span className="hint">キーチェーンに保存します</span></div>
      <input id="nas-password" type="password" className="field" style={{ width: 260 }} autoComplete="off"
        value={draft ?? (isSet ? MASK : "")}
        onFocus={() => { if (draft === null) setDraft(""); }}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => { if (draft === "") setDraft(null); else commit(); }}
        onKeyDown={(e) => { if (e.key === "Enter" && !isImeEnter(e)) commit(); }} />
    </div>
  );
}

export function SyncTab() {
  const [status, setStatus] = useState<SyncStatus | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [busy, setBusy] = useState<ReadonlySet<string>>(new Set());
  const inFlight = useRef(new Set<string>());

  const [guideOpen, setGuideOpen] = useState(false);
  const [nasTest, setNasTest] = useState<{ text: string; ok: boolean } | null>(null);
  const [confirmForget, setConfirmForget] = useState(false);
  const [confirmClear, setConfirmClear] = useState(false);
  const closeGuide = useCallback(() => setGuideOpen(false), []);

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

  const tailnetFailed = status?.tailnet.state === "failed";
  useEffect(() => { if (!tailnetFailed) setConfirmForget(false); }, [tailnetFailed]);

  if (!status) return <ErrorLine message={loadError} />;
  const isNas = status.backend === "nas";
  const usable = isNas || status.available;
  const tailnetRunning = status.tailnet.state === "running";
  const canSync = isNas ? tailnetRunning && status.nas_url !== "" : status.signed_in;
  const chooseBackend = (backend: string) => {
    if ((backend === "nas") === isNas) return;
    setNasTest(null);
    void run("backend", () => api.syncSetBackend(backend)).then((r) => { if (r.ok && backend === "nas" && status.nas_url === "") setGuideOpen(true); });
  };
  const nasSaved = status.nas_url !== "" || status.nas_user !== "" || status.nas_password_set;
  const saveNas = (name: string, url: string, user: string, password: string | null) => run(name, () => api.syncSetNas(url, user, password)).then((r) => { if (r.ok) setNasTest(null); return r; });
  return (
    <>
      <ErrorLine message={loadError} />
      <ErrorLine message={actionError} />
      <section className="section">
        <h2>保存先</h2>
        <p className="desc">{STORAGE_DESC}</p>
        <div className="setting-row">
          <div className="label"><span>保存先</span></div>
          <Segmented ariaLabel="保存先" value={isNas ? "nas" : "gdrive"} onChange={chooseBackend}
            options={[{ id: "gdrive", label: "Google ドライブ" }, { id: "nas", label: "NAS (WebDAV)" }]} />
        </div>
        {isNas ? (
          <>
            <div className="setting-row">
              <div className="label">
                <span>Tailscale</span>
                <span className="hint">NAS と同じ Tailscale アカウントでログインします。InfoDesk の通信だけが通り、Mac のネットワーク設定は変えません。</span>
              </div>
              <span className="cell-status">
                {tailnetRunning && <span className="dot dot-success" />}
                {tailnetText(status.tailnet)}
              </span>
              {tailnetRunning ? (
                <button className="btn" disabled={busy.has("tailnet")} onClick={() => run("tailnet", () => api.tailnetLogout())}>{busy.has("tailnet") ? "ログアウト中…" : "ログアウト"}</button>
              ) : (
                <button className="btn btn-primary" disabled={busy.has("tailnet")} onClick={() => run("tailnet", () => api.tailnetLogin())}>Tailscale にログイン</button>
              )}
            </div>
            {tailnetFailed && (
              <div className="setting-row">
                {confirmForget ? (
                  <div className="cell-confirm">
                    <span>Tailscale にもう一度ログインが必要になります。消しますか?</span>
                    <button className="btn btn-danger" disabled={busy.has("forget")} onClick={() => { void run("forget", () => api.tailnetForget()).then(() => setConfirmForget(false)); }}>消す</button>
                    <button className="btn" onClick={() => setConfirmForget(false)}>やめる</button>
                  </div>
                ) : (
                  <button className="btn" onClick={() => setConfirmForget(true)}>この Mac の Tailscale の状態を消す</button>
                )}
              </div>
            )}
            <SettingField id="nas-url" label="NAS の URL" hint="WebDAV のフォルダ。例: http://takeuchi-nas:5005/infodesk/" value={status.nas_url} width={360}
              onSave={(v) => saveNas("nas-url", v, status.nas_user, null)} />
            <SettingField id="nas-user" label="ユーザー名" value={status.nas_user}
              onSave={(v) => saveNas("nas-user", status.nas_url, v, null)} />
            <PasswordField isSet={status.nas_password_set} onSave={(pw) => saveNas("nas-password", status.nas_url, status.nas_user, pw)} />
            <div className="setting-row">
              <div className="label"><span>接続を確かめる</span><span className="hint">NAS に一時ファイルを書いて読み、消します</span></div>
              {nasTest && (
                <span className={`nas-test-result ${nasTest.ok ? "success" : "error"}`} data-testid="nas-test-result">
                  {nasTest.ok ? <CircleCheck size={14} /> : <CircleAlert size={14} />}{nasTest.text}
                </span>
              )}
              <button className="btn" disabled={busy.has("test")} onClick={() => run("test", () => api.syncTestNas().then((text) => { setNasTest({ text, ok: text === NAS_CHECK_OK }); }))}>{busy.has("test") ? "確かめています…" : "確かめる"}</button>
            </div>
            {nasSaved && (
              <div className="setting-row">
                {confirmClear ? (
                  <div className="cell-confirm">
                    <span>NAS の URL、ユーザー名、パスワードを消します。消しますか?</span>
                    <button className="btn btn-danger" disabled={busy.has("clear")} onClick={() => { void run("clear", () => api.syncClearNas()).then((r) => { setConfirmClear(false); if (r.ok) setNasTest(null); }); }}>消す</button>
                    <button className="btn" onClick={() => setConfirmClear(false)}>やめる</button>
                  </div>
                ) : (
                  <button className="btn" onClick={() => setConfirmClear(true)}>NAS の設定を消す</button>
                )}
              </div>
            )}
            <button className="link-btn" onClick={() => setGuideOpen(true)}><BookOpen size={14} />NAS の準備を見る (初めての方へ)</button>
          </>
        ) : !status.available ? (
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
      {usable && (
        <>
          <section className="section">
            <h2>同期</h2>
            <div className="setting-row">
              <div className="label"><span>同期する</span><span className="hint">収集のあと、ダイジェストを作ったあと、⌘R のときに同期します</span></div>
              <Toggle checked={status.enabled} ariaLabel="同期する" disabled={(!canSync && !status.enabled) || busy.has("enabled")} onChange={(v) => run("enabled", () => api.syncSetEnabled(v))} />
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
            <h2>保存先に置く期間</h2>
            <p className="desc">記事と日次ダイジェストは直近60日、週次と月次のダイジェストは直近400日。スマホの控えは消えません。</p>
          </section>
        </>
      )}
      {guideOpen && <NasGuideDialog onClose={closeGuide} />}
    </>
  );
}
