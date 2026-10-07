import { CircleAlert, Loader } from "lucide-react";
import { useState } from "react";
import { api } from "../../api";
import { CATEGORIES, errorMessage, type Category, type Source } from "../../types";

export function AddRssDialog({ onClose, onAdded }: { onClose: () => void; onAdded: (s: Source) => void }) {
  const [url, setUrl] = useState("");
  const [name, setName] = useState("");
  const [category, setCategory] = useState<Category>("tech_jp");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const canSubmit = url.trim().length > 0 && !busy;

  const submit = () => {
    setBusy(true);
    setError(null);
    api.addRssSource(url.trim(), name.trim() ? name.trim() : null, category)
      .then((s) => { onAdded(s); onClose(); })
      .catch((e) => { setError(errorMessage(e)); setBusy(false); });
  };

  return (
    <div className="overlay" onClick={onClose}>
      <div className="dialog" role="dialog" aria-label="RSSフィードを追加" onClick={(e) => e.stopPropagation()}>
        <h2>RSSフィードを追加</h2>
        <div className="row">
          <label htmlFor="rss-url">フィードURL</label>
          <input id="rss-url" className={`field ${error ? "field-error" : ""}`} value={url} onChange={(e) => { setUrl(e.target.value); setError(null); }} placeholder="https://" />
        </div>
        <div className="row">
          <label htmlFor="rss-name">表示名 (任意)</label>
          <input id="rss-name" className="field" value={name} onChange={(e) => setName(e.target.value)} placeholder="空欄の場合はフィードのタイトルを使います" />
        </div>
        <div className="row">
          <label htmlFor="rss-category">カテゴリー</label>
          <select id="rss-category" className="field" value={category} onChange={(e) => setCategory(e.target.value as Category)}>
            {CATEGORIES.map((c) => <option key={c.id} value={c.id}>{c.label}</option>)}
          </select>
        </div>
        <div className={`validation ${error ? "error" : ""}`} aria-live="polite">
          {busy && <><Loader size={14} className="spin" /><span>フィードを取得して検証しています…</span></>}
          {error && <><CircleAlert size={14} /><span>{error} このURLは保存されません。</span></>}
        </div>
        <div className="actions">
          <button className="btn" onClick={onClose}>キャンセル</button>
          <button className="btn btn-primary" disabled={!canSubmit} onClick={submit}>追加</button>
        </div>
      </div>
    </div>
  );
}
