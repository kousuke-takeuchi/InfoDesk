import { useEffect, useRef } from "react";
import { onEvent } from "../api";

/** Tauri のイベントを購読する。handler は常に最新のものが呼ばれる。 */
export function useTauriEvent(name: string, handler: () => void) {
  const latest = useRef(handler);
  latest.current = handler;
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    onEvent(name, () => latest.current()).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [name]);
}
