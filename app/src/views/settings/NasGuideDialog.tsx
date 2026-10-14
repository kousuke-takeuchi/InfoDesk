import { ExternalLink, FolderPlus, Network, Package, Plug, ShieldCheck, UserPlus, X } from "lucide-react";
import { useEffect, type ComponentType } from "react";
import { api } from "../../api";

const GUIDE_URL = "https://github.com/kousuke-takeuchi/InfoDesk/blob/master/docs/13-nas-setup.ja.md";

const STEPS: { icon: ComponentType<{ size?: number }>; title: string; body: string }[] = [
  { icon: Package, title: "DSM で WebDAV Server を入れる", body: "パッケージセンターで「WebDAV Server」を入れ、HTTP (ポート 5005) を有効にします。" },
  { icon: FolderPlus, title: "共有フォルダを作る", body: "コントロールパネル → 共有フォルダで「infodesk」を作ります。" },
  { icon: UserPlus, title: "InfoDesk 用のユーザーを作る", body: "コントロールパネル → ユーザーで新しいユーザーを作り、infodesk だけ「読み取り/書き込み」、ほかの共有フォルダは「アクセス不可」にします。" },
  { icon: Network, title: "NAS に Tailscale を入れる", body: "パッケージセンターで「Tailscale」を入れてログインし、NAS の名前 (例: takeuchi-nas) を控えます。" },
  { icon: Plug, title: "InfoDesk でつなぐ", body: "保存先に「NAS (WebDAV)」を選び、Tailscale にログイン (NAS と同じアカウント)。URL に http://NAS の名前:5005/infodesk/、ユーザー名とパスワードを入れて「確かめる」を押します。" },
];

export function NasGuideDialog({ onClose }: { onClose: () => void }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="overlay" onClick={onClose}>
      <div className="dialog nas-guide" role="dialog" aria-modal="true" aria-label="NAS の準備" onClick={(e) => e.stopPropagation()}>
        <div className="guide-head">
          <div>
            <h2>NAS の準備</h2>
            <p className="desc">Synology NAS を InfoDesk の保存先にします。最初の1回だけ行います。</p>
          </div>
          <button className="icon-btn" aria-label="ダイアログを閉じる" onClick={onClose}><X size={18} /></button>
        </div>
        <ol className="guide-steps">
          {STEPS.map((s, i) => (
            <li key={s.title}>
              <span className="step-num" aria-hidden="true">{i + 1}</span>
              <div>
                <div className="step-title"><s.icon size={14} /><strong>{s.title}</strong></div>
                <p className="desc">{s.body}</p>
              </div>
            </li>
          ))}
        </ol>
        <div className="guide-tip">
          <ShieldCheck size={16} />
          <span>Mac や iPhone に Tailscale のアプリは要りません。InfoDesk の中で Tailscale を使うので、ルーターのポートを開けず、ほかのアプリの通信にも影響しません。</span>
        </div>
        <div className="actions guide-actions">
          <button className="link-btn" onClick={() => { void api.openLink(GUIDE_URL).catch(() => {}); }}><ExternalLink size={14} />詳しい手順 (docs/13-nas-setup)</button>
          <button className="btn btn-primary" onClick={onClose}>閉じる</button>
        </div>
      </div>
    </div>
  );
}
