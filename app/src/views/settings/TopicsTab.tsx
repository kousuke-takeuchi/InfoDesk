import { Hash, Pencil, Plus, Trash2 } from "lucide-react";
import { Fragment, useCallback, useEffect, useRef, useState } from "react";
import { api, EVENTS } from "../../api";
import { ErrorLine } from "../../components/ErrorLine";
import { RevealCard } from "../../components/RevealCard";
import { useTauriEvent } from "../../hooks/useTauriEvent";
import { errorMessage, type SidebarCounts, type Topic } from "../../types";

type Draft = { id: number | null; name: string; keywords: string };

/** カンマ区切りを語に分ける。空の語と重複 (完全一致、先に出た方を残す) は除く。 */
function splitKeywords(s: string): string[] {
  return [...new Set(s.split(/[,、]/).map((k) => k.trim()).filter((k) => k.length > 0))];
}

export function TopicsTab() {
  const [topics, setTopics] = useState<Topic[]>([]);
  const [counts, setCounts] = useState<SidebarCounts | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  // 一覧の取得の失敗と操作の失敗は別々に持つ。イベントによる再取得が操作のエラーを消さないようにする。
  const [loadError, setLoadError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  // 削除の確認を出している行。同時に1行だけ。
  const [confirmingId, setConfirmingId] = useState<number | null>(null);

  const latestLoad = useRef(0);

  // 入力中の draft や削除の確認は別の state なので、再取得では閉じない。
  const load = useCallback(() => {
    // 最後に出した問い合わせの応答だけを反映する。追い越された応答は捨てる。
    const mine = ++latestLoad.current;
    Promise.all([api.listTopics(), api.sidebarCounts()])
      .then(([t, c]) => {
        if (latestLoad.current !== mine) return;
        setTopics(t); setCounts(c); setLoadError(null);
      })
      .catch((e) => { if (latestLoad.current === mine) setLoadError(errorMessage(e)); });
  }, []);
  useEffect(() => {
    load();
    // アンマウント後に届いた応答を捨てる
    return () => { latestLoad.current += 1; };
  }, [load]);
  // 収集や既読操作で未読数が変わる
  useTauriEvent(EVENTS.items, load);
  const saved = () => { setActionError(null); load(); };
  const run = (p: Promise<unknown>) => p.then(saved).catch((e) => setActionError(errorMessage(e)));

  const remove = (id: number) => {
    setConfirmingId(null);
    run(api.deleteTopic(id).then(() => setDraft((cur) => (cur?.id === id ? null : cur))));
  };

  const draftKeywords = draft ? splitKeywords(draft.keywords) : [];
  const canSave = draft !== null && draft.name.trim().length > 0 && draftKeywords.length > 0 && !saving;

  const save = () => {
    if (!draft || !canSave) return;
    setSaving(true);
    const p = draft.id === null ? api.addTopic(draft.name.trim(), draftKeywords) : api.updateTopic(draft.id, draft.name.trim(), draftKeywords);
    p.then(() => { setDraft(null); saved(); })
      .catch((e) => setActionError(errorMessage(e)))
      .finally(() => setSaving(false));
  };

  // 既存トピックの編集はその行の直下に、新規追加は一覧の末尾に出す
  const editor = draft && (
        <RevealCard key={draft.id ?? "new"}>
          <h2 style={{ fontSize: 13 }}>{draft.id === null ? "トピックを追加" : "トピックを編集"}</h2>
          <div className="row"><label htmlFor="topic-name">名前</label><input id="topic-name" className="field" value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} /></div>
          <div className="row" style={{ alignItems: "flex-start" }}>
            <label htmlFor="topic-kw" style={{ paddingTop: 6 }}>キーワード</label>
            <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: 6 }}>
              <input id="topic-kw" className="field" value={draft.keywords} onChange={(e) => setDraft({ ...draft, keywords: e.target.value })} placeholder="認知症, dementia, Alzheimer" />
              <span className="tiny text-tertiary">カンマ区切り。2文字以下の語も部分一致で照合します。</span>
              {draft.id !== null && <span className="tiny text-tertiary">キーワードを変えると、このトピックの要約は作り直しになります。</span>}
            </div>
          </div>
          <div className="actions">
            <button className="btn" onClick={() => setDraft(null)}>キャンセル</button>
            <button className="btn btn-primary" disabled={!canSave} onClick={save}>保存</button>
          </div>
        </RevealCard>
  );

  return (
    <div className="section">
      <div style={{ display: "flex", alignItems: "center" }}>
        <h2>トピック</h2>
        <div style={{ flex: 1 }} />
        <button className="btn btn-primary" onClick={() => setDraft({ id: null, name: "", keywords: "" })}><Plus size={14} /> トピックを追加</button>
      </div>
      <p className="desc">キーワードのいずれかを含む記事がトピックに属します。日本語と英語を併記すると両方の記事が集まります。英数字だけの語は単語単位で照合します。</p>
      <ErrorLine message={loadError} />
      <ErrorLine message={actionError} />
      <div className="table-head topics-grid"><span>トピック</span><span>キーワード</span><span>未読</span><span /></div>
      {topics.map((t) => (
        <Fragment key={t.id}>
        <div className={`table-row topics-grid ${confirmingId === t.id ? "confirming" : ""}`}>
          <span className="cell-topic" title={t.name}><Hash size={14} className="text-secondary" /><span className="truncate">{t.name}</span></span>
          <span className="cell-keywords">{t.keywords.map((k) => <span key={k} className="chip" title={k}>{k}</span>)}</span>
          <span className="small text-secondary">{counts?.topics[String(t.id)] ?? 0}</span>
          {/* 確認中も列の幅は変えない。操作列は空にし、確認は行の下に全幅で出す */}
          {confirmingId === t.id ? <span /> : (
            <div className="cell-actions">
              <button aria-label={`${t.name} を編集`} onClick={() => setDraft({ id: t.id, name: t.name, keywords: t.keywords.join(", ") })}><Pencil size={14} /></button>
              <button aria-label={`${t.name} を削除`} onClick={() => setConfirmingId(t.id)}><Trash2 size={14} /></button>
            </div>
          )}
          {confirmingId === t.id && (
            <div className="cell-confirm">
              <span>削除しますか? このトピックの要約も削除されます。</span>
              <button className="btn btn-danger" onClick={() => remove(t.id)}>削除する</button>
              <button className="btn" onClick={() => setConfirmingId(null)}>キャンセル</button>
            </div>
          )}
        </div>
        {draft?.id === t.id && editor}
        </Fragment>
      ))}
      {draft?.id === null && editor}
    </div>
  );
}
