import { CircleAlert, Loader } from "lucide-react";
import { useEffect, useState, type FormEvent, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { api } from "../../api";
import { CATEGORIES, errorMessage, type Category, type Source } from "../../types";
import { isImeEnter } from "./shared";

// IME の変換を確定する Enter でフォームを送信しない (WKWebView は確定の Enter でも暗黙の送信をする)
const blockImeEnter = (e: ReactKeyboardEvent) => { if (e.key === "Enter" && isImeEnter(e)) e.preventDefault(); };

export function AddRssDialog({ onClose, onAdded }: { onClose: () => void; onAdded: (s: Source) => void }) {
  const [url, setUrl] = useState("");
  const [name, setName] = useState("");
  const [category, setCategory] = useState<Category>("tech_jp");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const canSubmit = url.trim().length > 0 && !busy;

  // 検証中は閉じない。結果が届く前に閉じると、追加できたのか分からなくなる。
  const dismiss = () => { if (!busy) onClose(); };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape" && !busy) onClose(); };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [busy, onClose]);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!canSubmit) return;
    setBusy(true);
    setError(null);
    api.addRssSource(url.trim(), name.trim() ? name.trim() : null, category)
      .then((s) => { onAdded(s); onClose(); })
      .catch((err) => { setError(errorMessage(err)); setBusy(false); });
  };

  return (
    <div className="overlay" onClick={dismiss}>
      <form className="dialog" role="dialog" aria-label="RSSフィードを追加" onClick={(e) => e.stopPropagation()} onSubmit={submit}>
        <h2>RSSフィードを追加</h2>
        <div className="row">
          <label htmlFor="rss-url">フィードURL</label>
          <input id="rss-url" autoFocus className={`field ${error ? "field-error" : ""}`} value={url} onChange={(e) => { setUrl(e.target.value); setError(null); }} onKeyDown={blockImeEnter} placeholder="https://" />
        </div>
        <div className="row">
          <label htmlFor="rss-name">表示名 (任意)</label>
          <input id="rss-name" className="field" value={name} onChange={(e) => setName(e.target.value)} onKeyDown={blockImeEnter} placeholder="空欄の場合はフィードのタイトルを使います" />
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
          <button type="button" className="btn" disabled={busy} onClick={dismiss}>キャンセル</button>
          <button type="submit" className="btn btn-primary" disabled={!canSubmit}>追加</button>
        </div>
      </form>
    </div>
  );
}
