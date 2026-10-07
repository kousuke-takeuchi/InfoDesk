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
