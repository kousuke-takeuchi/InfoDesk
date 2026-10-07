import { Pencil, Plus, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { api } from "../../api";
import { ErrorLine } from "../../components/ErrorLine";
import { Toggle } from "../../components/Toggle";
import { formatClock } from "../../lib/dates";
import { CATEGORIES, categoryLabel, errorMessage, type Category, type Source } from "../../types";
import { AddRssDialog } from "./AddRssDialog";

function statusCell(s: Source, count: number) {
  if (!s.enabled) return <span className="cell-status"><i className="dot dot-muted" />無効</span>;
  if (s.last_error) return <span className="cell-status error"><i className="dot dot-danger" />{s.last_error}</span>;
  if (s.last_fetched_at) return <span className="cell-status"><i className="dot dot-success" />{count}件</span>;
  return <span className="cell-status"><i className="dot dot-muted" />未取得</span>;
}

export function SourcesTab() {
  const [sources, setSources] = useState<Source[]>([]);
  const [counts, setCounts] = useState<Record<string, number>>({});
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [editing, setEditing] = useState<Source | null>(null);
  // 削除の確認を出している行。同時に1行だけ。
  const [confirmingId, setConfirmingId] = useState<number | null>(null);

  const load = useCallback(() => {
    Promise.all([api.listSources(), api.sourceItemCounts()])
      .then(([s, c]) => { setSources(s); setCounts(c); setError(null); })
      .catch((e) => setError(errorMessage(e)));
  }, []);
  useEffect(load, [load]);
  const run = (p: Promise<unknown>) => p.then(load).catch((e) => setError(errorMessage(e)));

  const remove = (id: number) => {
    setConfirmingId(null);
    run(api.deleteSource(id).then(() => setEditing((cur) => (cur?.id === id ? null : cur))));
  };

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
      {sources.map((s) => {
        const confirming = confirmingId === s.id;
        return (
          <div key={s.id} className={`table-row sources-grid ${confirming ? "confirming" : ""}`}>
            <Toggle checked={s.enabled} ariaLabel={`${s.name} を${s.enabled ? "無効" : "有効"}にする`} onChange={(v) => run(api.setSourceEnabled(s.id, v))} />
            <div className="cell-name">
              <span style={{ fontWeight: 500, color: s.enabled ? undefined : "var(--text-tertiary)" }}>{s.name}</span>
              <span className="url">{s.config}</span>
            </div>
            <span className="small text-secondary">{categoryLabel(s.category)}</span>
            <span className="small text-secondary">{s.last_fetched_at ? formatClock(s.last_fetched_at) : "—"}</span>
            {statusCell(s, counts[String(s.id)] ?? 0)}
            {confirming ? (
              <div className="cell-confirm">
                <span>削除しますか?</span>
                <button className="btn btn-danger" onClick={() => remove(s.id)}>削除する</button>
                <button className="btn" onClick={() => setConfirmingId(null)}>キャンセル</button>
              </div>
            ) : (
              <div className="cell-actions">
                <button aria-label={`${s.name} を編集`} onClick={() => setEditing(s)}><Pencil size={14} /></button>
                <button aria-label={`${s.name} を削除`} onClick={() => setConfirmingId(s.id)}><Trash2 size={14} /></button>
              </div>
            )}
          </div>
        );
      })}
      <div className="small text-tertiary" style={{ padding: "12px 0" }}>{sources.length}件のソース · 有効 {enabled}件 · エラー {errors}件</div>
      {/* key で行ごとに作り直す。開いたまま別の行を選んでも前の行の入力値を引き継がない。 */}
      {editing && <EditSourceCard key={editing.id} source={editing} onClose={() => setEditing(null)} onSaved={() => { setEditing(null); load(); }} onError={setError} />}
      {adding && <AddRssDialog onClose={() => setAdding(false)} onAdded={() => load()} />}
    </div>
  );
}

function EditSourceCard({ source, onClose, onSaved, onError }: { source: Source; onClose: () => void; onSaved: () => void; onError: (m: string) => void }) {
  const [name, setName] = useState(source.name);
  const [category, setCategory] = useState<Category>(source.category);
  const [saving, setSaving] = useState(false);
  const isRss = source.kind === "rss";

  // config は検証なしでは書き換えない。受け取った値をそのまま送り返す。
  const save = () => {
    setSaving(true);
    api.updateSource(source.id, name.trim(), category, source.config)
      .then(onSaved)
      .catch((e) => { setSaving(false); onError(errorMessage(e)); });
  };

  return (
    <div className="card">
      <h2 style={{ fontSize: 13 }}>ソースを編集</h2>
      <div className="row"><label htmlFor="src-name">表示名</label><input id="src-name" className="field" value={name} onChange={(e) => setName(e.target.value)} /></div>
      <div className="row"><label htmlFor="src-cat">カテゴリー</label>
        <select id="src-cat" className="field" value={category} onChange={(e) => setCategory(e.target.value as Category)}>
          {CATEGORIES.map((c) => <option key={c.id} value={c.id}>{c.label}</option>)}
        </select>
      </div>
      <div className="row readonly">
        <span className="row-label">{isRss ? "フィードURL" : "設定値"}</span>
        <div className="readonly-value">
          <span className="value">{source.config}</span>
          {isRss && <span className="tiny text-tertiary">URLを変える場合は、削除してから追加し直してください。</span>}
        </div>
      </div>
      <div className="actions">
        <button className="btn" onClick={onClose}>キャンセル</button>
        <button className="btn btn-primary" disabled={!name.trim() || saving} onClick={save}>保存</button>
      </div>
    </div>
  );
}
