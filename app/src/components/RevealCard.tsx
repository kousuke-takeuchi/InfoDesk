import { useEffect, useRef, type ReactNode } from "react";

/** 開いたときに見える位置までスクロールするカード。一覧の下の方の行で開いても画面外に残らない。 */
export function RevealCard({ children }: { children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  // jsdom には scrollIntoView が無いので、あるときだけ呼ぶ
  useEffect(() => { ref.current?.scrollIntoView?.({ block: "nearest" }); }, []);
  return <div className="card" ref={ref}>{children}</div>;
}
