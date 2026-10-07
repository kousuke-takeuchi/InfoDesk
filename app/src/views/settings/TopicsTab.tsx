import { Hash, Pencil, Plus, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { api } from "../../api";
import { ErrorLine } from "../../components/ErrorLine";
import { errorMessage, type SidebarCounts, type Topic } from "../../types";

type Draft = { id: number | null; name: string; keywords: string };

function splitKeywords(s: string): string[] {
  return s.split(/[,、]/).map((k) => k.trim()).filter((k) => k.length > 0);
}

export function TopicsTab() {
  const [topics, setTopics] = useState<Topic[]>([]);
  const [counts, setCounts] = useState<SidebarCounts | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    Promise.all([api.listTopics(), api.sidebarCounts()])
      .then(([t, c]) => { setTopics(t); setCounts(c); setError(null); })
      .catch((e) => setError(errorMessage(e)));
  }, []);
  useEffect(load, [load]);
  const run = (p: Promise<unknown>) => p.then(load).catch((e) => setError(errorMessage(e)));

  const save = () => {
    if (!draft) return;
    const keywords = splitKeywords(draft.keywords);
    const p = draft.id === null ? api.addTopic(draft.name.trim(), keywords) : api.updateTopic(draft.id, draft.name.trim(), keywords);
    p.then(() => { setDraft(null); load(); }).catch((e) => setError(errorMessage(e)));
  };

  return (
    <div className="section">
      <div style={{ display: "flex", alignItems: "center" }}>
        <h2>トピック</h2>
        <div style={{ flex: 1 }} />
        <button className="btn btn-primary" onClick={() => setDraft({ id: null, name: "", keywords: "" })}><Plus size={14} /> トピックを追加</button>
      </div>
      <p className="desc">キーワードのいずれかを含む記事がトピックに属します。日本語と英語を併記すると両方の記事が集まります。英数字だけの語は単語単位で照合します。</p>
      <ErrorLine message={error} />
      <div className="table-head topics-grid"><span>トピック</span><span>キーワード</span><span>未読</span><span /></div>
      {topics.map((t) => (
        <div key={t.id} className="table-row topics-grid">
          <span style={{ display: "flex", gap: 8, alignItems: "center", fontWeight: 500 }}><Hash size={14} className="text-secondary" />{t.name}</span>
          <span style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>{t.keywords.map((k) => <span key={k} className="chip">{k}</span>)}</span>
          <span className="small text-secondary">{counts?.topics[String(t.id)] ?? 0}</span>
          <div className="cell-actions">
            <button aria-label={`${t.name} を編集`} onClick={() => setDraft({ id: t.id, name: t.name, keywords: t.keywords.join(", ") })}><Pencil size={14} /></button>
            <button aria-label={`${t.name} を削除`} onClick={() => run(api.deleteTopic(t.id))}><Trash2 size={14} /></button>
          </div>
        </div>
      ))}
      {draft && (
        <div className="card">
          <h2 style={{ fontSize: 13 }}>{draft.id === null ? "トピックを追加" : "トピックを編集"}</h2>
          <div className="row"><label htmlFor="topic-name">名前</label><input id="topic-name" className="field" value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} /></div>
          <div className="row" style={{ alignItems: "flex-start" }}>
            <label htmlFor="topic-kw" style={{ paddingTop: 6 }}>キーワード</label>
            <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: 6 }}>
              <input id="topic-kw" className="field" value={draft.keywords} onChange={(e) => setDraft({ ...draft, keywords: e.target.value })} placeholder="認知症, dementia, Alzheimer" />
              <span className="tiny text-tertiary">カンマ区切り。2文字以下の語も部分一致で照合します。</span>
            </div>
          </div>
          <div className="actions">
            <button className="btn" onClick={() => setDraft(null)}>キャンセル</button>
            <button className="btn btn-primary" disabled={!draft.name.trim()} onClick={save}>保存</button>
          </div>
        </div>
      )}
    </div>
  );
}
