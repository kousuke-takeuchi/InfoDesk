import type { ReasoningMessagePartProps } from "@assistant-ui/react";
import { ChevronRight } from "lucide-react";
import { useContext, useState } from "react";
import { WorkCollapsedContext } from "./WorkSummary";

export function Reasoning({ text, status }: ReasoningMessagePartProps) {
  const collapsed = useContext(WorkCollapsedContext);
  const [open, setOpen] = useState(false);
  if (collapsed || !text.trim()) return null;
  const thinking = status.type === "running";
  return (
    <div className="reasoning">
      <button className="reasoning-head" aria-expanded={open} onClick={() => setOpen((v) => !v)}>
        <ChevronRight size={13} className={open ? "open" : ""} aria-hidden />{thinking ? "考え中…" : "考えた内容"}
      </button>
      {open && <div className="reasoning-body">{text}</div>}
    </div>
  );
}
