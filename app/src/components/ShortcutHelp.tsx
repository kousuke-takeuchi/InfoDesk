import { X } from "lucide-react";
import { useEffect, useRef } from "react";

const GROUPS: { title: string; rows: [string, string][] }[] = [
  {
    title: "全体",
    rows: [
      ["⌘R / Ctrl+R", "今すぐ収集 (表示も更新)"],
      ["1 / 2", "ダイジェスト / フィード"],
      ["d / w / m", "期間: 今日 / 週 (フィードは1週間) / 月 (フィードは1ヶ月)"],
      ["[ / ]", "サイドバーの前 / 次の項目"],
      ["0", "すべてを選択"],
      ["⌘J / Ctrl+J", "チャットパネルの開閉"],
      ["⌘, / Ctrl+,", "設定を開く"],
      ["?", "ヘルプ (この画面)"],
    ],
  },
  {
    title: "フィード",
    rows: [
      ["j / k", "次の記事 / 前の記事"],
      ["Enter", "記事を開く"],
      ["u", "既読 / 未読を切り替え"],
      ["s", "スターを切り替え"],
      ["/", "検索欄にフォーカス"],
      ["Shift+A", "すべて既読にする"],
    ],
  },
];

export function ShortcutHelp({ onClose }: { onClose: () => void }) {
  const close = useRef(onClose);
  close.current = onClose;
  const button = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    button.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      close.current();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  return (
    <div className="shortcut-backdrop" onClick={onClose}>
      <div className="shortcut-help" role="dialog" aria-modal="true" aria-label="ショートカット" onClick={(e) => e.stopPropagation()}>
        <div className="shortcut-head">
          <h2>ショートカット</h2>
          <button ref={button} className="btn btn-ghost" aria-label="閉じる" onClick={onClose}><X size={14} /></button>
        </div>
        {GROUPS.map((g) => (
          <section key={g.title}>
            <h3>{g.title}</h3>
            <dl>
              {g.rows.map(([keys, label]) => (
                <div key={keys} className="shortcut-row">
                  <dt><kbd>{keys}</kbd></dt>
                  <dd>{label}</dd>
                </div>
              ))}
            </dl>
          </section>
        ))}
      </div>
    </div>
  );
}
