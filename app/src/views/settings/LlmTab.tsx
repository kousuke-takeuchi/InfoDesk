import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../../api";
import { ErrorLine } from "../../components/ErrorLine";
import { errorMessage, type ProbeResult, type Settings } from "../../types";

const PROVIDERS = [{ id: "ollama", label: "Ollama" }, { id: "claude", label: "Claude (claude CLI)" }];

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

  /** name ごとに同時実行を1件に制限する。成功したら actionError を消して設定を再取得し、成否を返す。 */
  const act = useCallback((name: string, fn: () => Promise<unknown>) => {
    if (inFlight.current.has(name)) return Promise.resolve(false);
    inFlight.current.add(name);
    setBusy(new Set(inFlight.current));
    return fn()
      .then(() => { setActionError(null); return load().then(() => true); })
      .catch((e) => { setActionError(errorMessage(e)); return false; })
      .finally(() => { inFlight.current.delete(name); setBusy(new Set(inFlight.current)); });
  }, [load]);

  const save = useCallback((key: string, value: string) => act(`set:${key}`, () => api.setSetting(key, value)), [act]);
  return { settings, loadError, actionError, busy, act, save };
}

/**
 * blur または Enter で保存するテキスト欄。
 * validate が文言を返したら保存せず欄の横に表示する。normalize は保存前の整形。
 * 保存に失敗しても入力値は残す (value が変わったときだけ draft を差し替える)。
 */
export function SettingField({ id, label, value, onSave, hint, type = "text", width = 260, validate, normalize }: {
  id: string; label: string; value: string; onSave: (v: string) => Promise<boolean> | void; hint?: string; type?: string; width?: number;
  validate?: (v: string) => string | null; normalize?: (v: string) => string;
}) {
  const [draft, setDraft] = useState(value);
  const [invalid, setInvalid] = useState<string | null>(null);
  const sent = useRef<string | null>(null);
  useEffect(() => { setDraft(value); setInvalid(null); sent.current = null; }, [value]);
  const commit = () => {
    if (draft === value) { setInvalid(null); return; }
    const problem = validate?.(draft) ?? null;
    if (problem) { setInvalid(problem); return; }
    setInvalid(null);
    const next = normalize ? normalize(draft) : draft;
    setDraft(next);
    // Enter の直後に blur でも commit が走るため、保存済みまたは送信中の同じ値は再送しない。失敗したら再試行できるよう戻す
    if (next === value || next === sent.current) return;
    sent.current = next;
    void Promise.resolve(onSave(next)).then((ok) => { if (ok === false && sent.current === next) sent.current = null; });
  };
  return (
    <div className="setting-row">
      <div className="label"><label htmlFor={id}>{label}</label>{hint && <span className="hint">{hint}</span>}</div>
      {invalid && <span className="cell-status error" role="alert">{invalid}</span>}
      <input id={id} type={type} className="field" style={{ width }} value={draft} aria-invalid={invalid ? true : undefined}
        onChange={(e) => { setDraft(e.target.value); sent.current = null; }} onBlur={commit}
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
  const start = () => {
    setBusy(true);
    run().then(setResult).catch((e) => setResult({ ok: false, message: errorMessage(e), millis: 0 })).finally(() => setBusy(false));
  };
  return (
    <div className="setting-row">
      <div className="label"><span>接続テスト</span></div>
      {result && (
        <span className={`cell-status ${result.ok ? "" : "error"}`}>
          <i className={`dot ${result.ok ? "dot-success" : "dot-danger"}`} />
          {result.message} · {(result.millis / 1000).toFixed(1)}秒
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
