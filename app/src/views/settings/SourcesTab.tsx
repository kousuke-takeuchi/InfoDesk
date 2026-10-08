import { Pencil, Plus, Trash2 } from "lucide-react";
import { Fragment, useCallback, useEffect, useRef, useState } from "react";
import { api, EVENTS } from "../../api";
import { ErrorLine } from "../../components/ErrorLine";
import { RevealCard } from "../../components/RevealCard";
import { Toggle } from "../../components/Toggle";
import { useTauriEvent } from "../../hooks/useTauriEvent";
import { formatClock } from "../../lib/dates";
import { CATEGORIES, categoryLabel, errorMessage, type Category, type Source } from "../../types";
import { AddRssDialog } from "./AddRssDialog";

function statusCell(s: Source, count: number) {
  if (!s.enabled) return <span className="cell-status"><i className="dot dot-muted" />無効</span>;
  if (s.last_error) return <span className="cell-status error multiline" title={s.last_error}><i className="dot dot-danger" /><span className="clamp-3">{s.last_error}</span></span>;
  if (s.last_fetched_at) return <span className="cell-status"><i className="dot dot-success" />{count}件</span>;
  return <span className="cell-status"><i className="dot dot-muted" />未取得</span>;
}

export function SourcesTab() {
  const [sources, setSources] = useState<Source[]>([]);
  const [counts, setCounts] = useState<Record<string, number>>({});
  // 一覧の取得の失敗と操作の失敗は別々に持つ。イベントによる再取得が操作のエラーを消さないようにする。
  const [loadError, setLoadError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [editing, setEditing] = useState<Source | null>(null);
  // 削除の確認を出している行。同時に1行だけ。
  const [confirmingId, setConfirmingId] = useState<number | null>(null);

  const latestLoad = useRef(0);

  // 編集カードや削除の確認は別の state なので、再取得では閉じない。
  const load = useCallback(() => {
    // 最後に出した問い合わせの応答だけを反映する。status-changed は続けて届くので、追い越された応答は捨てる。
    const mine = ++latestLoad.current;
    Promise.all([api.listSources(), api.sourceItemCounts()])
      .then(([s, c]) => {
        if (latestLoad.current !== mine) return;
        setSources(s); setCounts(c); setLoadError(null);
      })
      .catch((e) => { if (latestLoad.current === mine) setLoadError(errorMessage(e)); });
  }, []);
  useEffect(() => {
    load();
    // アンマウント後に届いた応答を捨てる
    return () => { latestLoad.current += 1; };
  }, [load]);
  // 収集が終わると最終取得・件数・エラーが変わる。全ソースが失敗した収集は status-changed しか出さない。
  useTauriEvent(EVENTS.status, load);
  useTauriEvent(EVENTS.items, load);
  const saved = () => { setActionError(null); load(); };
  const run = (p: Promise<unknown>) => p.then(saved).catch((e) => setActionError(errorMessage(e)));

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
      <ErrorLine message={loadError} />
      <ErrorLine message={actionError} />
      <div className="table-head sources-grid"><span /><span>ソース</span><span>カテゴリー</span><span>最終取得</span><span>状態</span><span /></div>
      {sources.map((s) => {
        const confirming = confirmingId === s.id;
        return (
          <Fragment key={s.id}>
          <div className={`table-row sources-grid ${confirming ? "confirming" : ""}`}>
            <Toggle checked={s.enabled} ariaLabel={`${s.name} を${s.enabled ? "無効" : "有効"}にする`} onChange={(v) => run(api.setSourceEnabled(s.id, v))} />
            <div className="cell-name">
              <span className="name" title={s.name} style={{ color: s.enabled ? undefined : "var(--text-tertiary)" }}>{s.name}</span>
              <span className="url" title={s.config}>{s.config}</span>
            </div>
            <span className="small text-secondary">{categoryLabel(s.category)}</span>
            <span className="small text-secondary">{s.last_fetched_at ? formatClock(s.last_fetched_at) : "—"}</span>
            {statusCell(s, counts[String(s.id)] ?? 0)}
            {/* 確認中も列の幅は変えない。操作列は空にし、確認は行の下に全幅で出す */}
            {confirming ? <span /> : (
              <div className="cell-actions">
                <button aria-label={`${s.name} を編集`} onClick={() => setEditing(s)}><Pencil size={14} /></button>
                <button aria-label={`${s.name} を削除`} onClick={() => setConfirmingId(s.id)}><Trash2 size={14} /></button>
              </div>
            )}
            {confirming && (
              <div className="cell-confirm">
                <span>削除しますか?</span>
                <button className="btn btn-danger" onClick={() => remove(s.id)}>削除する</button>
                <button className="btn" onClick={() => setConfirmingId(null)}>キャンセル</button>
              </div>
            )}
          </div>
          {/* 編集カードは編集中の行の直下に出す (一覧の末尾だと行数が多いとき画面外になる)。key で行ごとに作り直し、前の行の入力値を引き継がない */}
          {editing?.id === s.id && <EditSourceCard key={editing.id} source={editing} onClose={() => setEditing(null)} onSaved={() => { setEditing(null); saved(); }} onError={setActionError} />}
          </Fragment>
        );
      })}
      <div className="small text-tertiary" style={{ padding: "12px 0" }}>{sources.length}件のソース · 有効 {enabled}件 · エラー {errors}件</div>
      {adding && <AddRssDialog onClose={() => setAdding(false)} onAdded={saved} />}
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
    <RevealCard>
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
    </RevealCard>
  );
}
