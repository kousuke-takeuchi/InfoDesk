import { useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { ArrowLeft, ArrowRight, ExternalLink, RotateCw, X } from "lucide-react";
import { api, EVENTS } from "../api";

interface ReaderState { url: string; title: string }

function hostOf(url: string): string {
  try { return new URL(url).host; } catch { return ""; }
}

/** 閲覧ウィンドウ上部のツールバー。ページの状態は Rust 側が持ち、イベントを受けたら取り直す。 */
export function ReaderToolbar() {
  const [state, setState] = useState<ReaderState>({ url: "", title: "" });

  useEffect(() => {
    let alive = true;
    const refresh = () => {
      api.readerState().then((s) => { if (alive) setState(s); }).catch(() => {});
    };
    refresh();
    const unlisten = getCurrentWebview().listen(EVENTS.reader, refresh);
    return () => {
      alive = false;
      void unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);

  const host = hostOf(state.url);
  return (
    <div className="reader-toolbar">
      <div className="reader-nav">
        <button aria-label="戻る" title="戻る" onClick={() => void api.readerBack().catch(() => {})}><ArrowLeft size={16} /></button>
        <button aria-label="進む" title="進む" onClick={() => void api.readerForward().catch(() => {})}><ArrowRight size={16} /></button>
        <button aria-label="再読み込み" title="再読み込み" onClick={() => void api.readerReload().catch(() => {})}><RotateCw size={16} /></button>
      </div>
      <div className="reader-page" title={state.url}>
        <span className="reader-title">{state.title || host}</span>
        {host && state.title && <span className="reader-host">{host}</span>}
      </div>
      <div className="reader-actions">
        <button aria-label="ブラウザで開く" title="ブラウザで開く" onClick={() => void api.readerOpenExternal().catch(() => {})}><ExternalLink size={16} /></button>
        <button aria-label="閉じる" title="閉じる" onClick={() => void api.readerClose().catch(() => {})}><X size={16} /></button>
      </div>
    </div>
  );
}
