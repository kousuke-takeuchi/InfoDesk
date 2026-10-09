import { MessageSquare } from "lucide-react";

export function ChatToggle({ open, onToggle }: { open: boolean; onToggle: () => void }) {
  return (
    <button className="btn btn-ghost icon-toggle" aria-label={open ? "チャットを閉じる" : "チャットを開く"} aria-pressed={open} onClick={onToggle}>
      <MessageSquare size={15} />
    </button>
  );
}
