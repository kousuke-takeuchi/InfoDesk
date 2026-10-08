import { useState } from "react";
import { api } from "../../api";
import { ErrorLine } from "../../components/ErrorLine";
import { errorMessage, type ProbeResult } from "../../types";
import { SettingField, useSettingsState, withStoredOption } from "./shared";

const PROVIDERS = [{ id: "ollama", label: "Ollama" }, { id: "claude", label: "Claude (claude CLI)" }];

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
