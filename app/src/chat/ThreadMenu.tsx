import { Trash2 } from "lucide-react";
import { useState } from "react";
import type { ChatThread as Thread } from "../types";

interface Props {
  threads: Thread[];
  currentId: number | null;
  onSelect(id: number): void;
  onDelete(id: number): void;
}

export function ThreadMenu({ threads, currentId, onSelect, onDelete }: Props) {
  const [confirming, setConfirming] = useState<number | null>(null);
  if (threads.length === 0) return <div className="thread-menu empty text-secondary">保存したスレッドはありません</div>;
  return (
    <ul className="thread-menu" aria-label="スレッド">
      {threads.map((t) => (
        <li key={t.id} className={t.id === currentId ? "current" : undefined}>
          {confirming === t.id ? (
            <div className="thread-confirm">
              <span>削除しますか?</span>
              <button className="btn btn-danger" onClick={() => { setConfirming(null); onDelete(t.id); }}>削除する</button>
              <button className="btn" onClick={() => setConfirming(null)}>キャンセル</button>
            </div>
          ) : (
            <>
              <button className="thread-item" onClick={() => onSelect(t.id)}>{t.title}</button>
              <button className="icon-btn" aria-label={`${t.title} を削除`} onClick={() => setConfirming(t.id)}><Trash2 size={13} /></button>
            </>
          )}
        </li>
      ))}
    </ul>
  );
}
