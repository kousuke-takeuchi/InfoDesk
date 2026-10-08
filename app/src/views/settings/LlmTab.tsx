import { useCallback, useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { api } from "../../api";
import { ErrorLine } from "../../components/ErrorLine";
import { errorMessage, type ProbeResult, type Settings } from "../../types";

const PROVIDERS = [{ id: "ollama", label: "Ollama" }, { id: "claude", label: "Claude (claude CLI)" }];

export interface SelectOption { id: string; label: string }
/** 操作の結果。error は失敗したときのバックエンドの文言 (多重実行で見送ったときは ok: false, error: null)。 */
export interface ActionResult { ok: boolean; error: string | null }

/**
 * 保存済みの値が選択肢に無いとき、その値を選択肢の末尾に足す。
 * こうしないと select は先頭の選択肢を表示し、保存されていない値を保存済みのように見せてしまう。
 */
export function withStoredOption(options: SelectOption[], stored: string, labelFor: (v: string) => string = (v) => v): SelectOption[] {
  if (options.some((o) => o.id === stored)) return options;
  return [...options, { id: stored, label: stored === "" ? "未設定" : labelFor(stored) }];
}

/**
 * 設定タブ共通の状態。loadError (get_settings の失敗) と actionError (保存や操作の失敗) は別々に持つ。
 * actionError は次の操作が成功したときだけ消え、再取得の成否では変わらない。
 */
export function useSettingsState() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [busy, setBusy] = useState<ReadonlySet<string>>(new Set());
  const inFlight = useRef(new Set<string>());

  const load = useCallback(
    () => api.getSettings().then((s) => { setSettings(s); setLoadError(null); }).catch((e) => setLoadError(errorMessage(e))),
    [],
  );
  useEffect(() => { void load(); }, [load]);

  /** name ごとに同時実行を1件に制限する。成功したら actionError を消して設定を再取得し、結果を返す。 */
  const attempt = useCallback((name: string, fn: () => Promise<unknown>): Promise<ActionResult> => {
    if (inFlight.current.has(name)) return Promise.resolve({ ok: false, error: null });
    inFlight.current.add(name);
    setBusy(new Set(inFlight.current));
    return fn()
      .then(() => { setActionError(null); return load().then(() => ({ ok: true, error: null })); })
      .catch((e) => { const error = errorMessage(e); setActionError(error); return { ok: false, error }; })
      .finally(() => { inFlight.current.delete(name); setBusy(new Set(inFlight.current)); });
  }, [load]);

  const act = useCallback((name: string, fn: () => Promise<unknown>) => attempt(name, fn).then((r) => r.ok), [attempt]);
  const save = useCallback((key: string, value: string) => attempt(`set:${key}`, () => api.setSetting(key, value)), [attempt]);
  return { settings, loadError, actionError, busy, act, save };
}

/**
 * IME の変換を確定する Enter かどうか。WKWebView では確定の Enter が isComposing === false,
 * keyCode === 229 で届くことがあるので両方を見る。
 */
export function isImeEnter(e: KeyboardEvent): boolean {
  return e.nativeEvent.isComposing || e.keyCode === 229;
}

/**
 * blur または Enter で保存するテキスト欄。
 * validate が文言を返したら保存せず欄の横に表示する。normalize は保存前の整形。
 * 保存に失敗しても入力値は残す (value が変わったときだけ draft を差し替える)。
 * 保存の失敗は欄の横にも出し、この欄の保存が成功するか入力値を保存済みの値に戻すまで残す
 * (上部の actionError は別の操作の成功で消えるため、それだけでは失敗した欄が分からなくなる)。
 */
export function SettingField({ id, label, value, onSave, hint, type = "text", width = 260, validate, normalize }: {
  id: string; label: string; value: string; onSave: (v: string) => Promise<ActionResult>; hint?: string; type?: string; width?: number;
  validate?: (v: string) => string | null; normalize?: (v: string) => string;
}) {
  const [draft, setDraft] = useState(value);
  const [invalid, setInvalid] = useState<string | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const sent = useRef<string | null>(null);
  // 保存済みの値が変わったときだけ、レンダー中に draft を差し替える。
  // effect で差し替えると、マウント直後の入力のあとに effect が走って入力を上書きすることがある。
  const [seen, setSeen] = useState(value);
  if (seen !== value) {
    setSeen(value);
    setDraft(value);
    setInvalid(null);
    setFailed(null);
  }
  // ref はレンダー中に書けないので layout effect で消す。コミット内で同期に走るので、入力のあとに回ることはない
  useLayoutEffect(() => { sent.current = null; }, [value]);
  const commit = () => {
    if (draft === value) { setInvalid(null); setFailed(null); return; }
    const problem = validate?.(draft) ?? null;
    if (problem) { setInvalid(problem); return; }
    setInvalid(null);
    const next = normalize ? normalize(draft) : draft;
    setDraft(next);
    if (next === value) { setFailed(null); return; }
    // Enter の直後に blur でも commit が走るため、送信中または送信済みの同じ値は再送しない。失敗したら再試行できるよう戻す
    if (next === sent.current) return;
    sent.current = next;
    void onSave(next).then((r) => {
      if (sent.current !== next) return; // 送信後に入力が変わった。結果はその新しい入力のものではない
      if (r.ok) { setFailed(null); return; }
      sent.current = null;
      if (r.error !== null) setFailed(r.error);
    });
  };
  const errorId = `${id}-error`;
  return (
    <div className="setting-row">
      <div className="label"><label htmlFor={id}>{label}</label>{hint && <span className="hint">{hint}</span>}</div>
      {invalid && <span className="cell-status error" role="alert">{invalid}</span>}
      {/* 保存の失敗は上部の ErrorLine が読み上げるので、ここは role="alert" にせず欄の説明として結び付ける */}
      {!invalid && failed && <span id={errorId} className="cell-status error">{failed}</span>}
      <input id={id} type={type} className="field" style={{ width }} value={draft} aria-invalid={invalid || failed ? true : undefined}
        aria-describedby={!invalid && failed ? errorId : undefined}
        onChange={(e) => { setDraft(e.target.value); sent.current = null; if (e.target.value === value) setFailed(null); }} onBlur={commit}
        onKeyDown={(e) => { if (e.key === "Enter" && !isImeEnter(e)) commit(); }} />
    </div>
  );
}

function ProviderRow({ id, label, hint, value, onChange }: { id: string; label: string; hint: string; value: string; onChange: (v: string) => void }) {
  return (
    <div className="setting-row">
      <div className="label"><label htmlFor={id}>{label}</label><span className="hint">{hint}</span></div>
      <select id={id} className="field" style={{ width: 180 }} value={value} onChange={(e) => onChange(e.target.value)}>
        {withStoredOption(PROVIDERS, value).map((p) => <option key={p.id} value={p.id}>{p.label}</option>)}
      </select>
    </div>
  );
}

/** millis が null なのはコマンド自体が失敗したとき (計測値が無いので所要時間は出さない)。 */
function Probe({ label, run }: { label: string; run: () => Promise<ProbeResult> }) {
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; message: string; millis: number | null } | null>(null);
  const start = () => {
    setBusy(true);
    setResult(null);
    run().then(setResult).catch((e) => setResult({ ok: false, message: errorMessage(e), millis: null })).finally(() => setBusy(false));
  };
  return (
    <div className="setting-row">
      <div className="label"><span>接続テスト</span></div>
      {result && (
        <span className={`cell-status ${result.ok ? "" : "error"}`}>
          <i className={`dot ${result.ok ? "dot-success" : "dot-danger"}`} />
          {result.millis === null ? result.message : `${result.message} · ${(result.millis / 1000).toFixed(1)}秒`}
        </span>
      )}
      <button className="btn" disabled={busy} onClick={start}>
        {busy ? "テスト中…" : `${label} をテスト`}
      </button>
    </div>
  );
}

export function LlmTab() {
  const { settings, loadError, actionError, save } = useSettingsState();
  if (!settings) return <ErrorLine message={loadError} />;
  const g = (k: string) => settings[k] ?? "";
  return (
    <>
      <ErrorLine message={loadError} />
      <ErrorLine message={actionError} />
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
