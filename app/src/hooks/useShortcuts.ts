import { useEffect, useRef, type MutableRefObject } from "react";
import { matchShortcut, type ShortcutAction } from "../lib/shortcuts";

/** ビューが登録する、ビュー固有のショートカットの実装。登録の無いアクションは何もしない。 */
export type ViewActions = Partial<Record<ShortcutAction, () => void>>;
export type ViewActionsRef = MutableRefObject<ViewActions>;

const DIALOG = '[role="dialog"][aria-modal="true"]';

/**
 * window の keydown を一つだけ受け、global のアクションは handlers で、
 * ビュー固有のものは viewActions に登録された関数で処理する。処理したキーだけ preventDefault する。
 */
export function useShortcuts(inSettings: boolean, handlers: ViewActions, viewActions: ViewActionsRef) {
  const latest = useRef({ inSettings, handlers });
  latest.current = { inSettings, handlers };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const { inSettings, handlers } = latest.current;
      const action = matchShortcut(e, { inSettings, dialogOpen: document.querySelector(DIALOG) !== null });
      if (!action) return;
      const run = handlers[action] ?? (inSettings ? undefined : viewActions.current[action]);
      if (!run) return;
      e.preventDefault();
      run();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [viewActions]);
}
