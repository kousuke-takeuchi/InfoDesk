import { useEffect, useRef } from "react";
import { localDateString } from "../lib/dates";

const DEBOUNCE_MS = 150;
const DATE_CHECK_MS = 60_000;

/**
 * ウィンドウが戻ってきたとき (focus / visible) と、ローカル日付が変わったときに onStale を呼ぶ。
 * 常駐アプリで古い表示が残るのを防ぐ。focus と visibilitychange が続けて来ても1回にまとめる。
 */
export function useStaleRefresh(onStale: () => void) {
  const latest = useRef(onStale);
  latest.current = onStale;
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    const fire = () => {
      if (timer !== null) clearTimeout(timer);
      timer = setTimeout(() => { timer = null; latest.current(); }, DEBOUNCE_MS);
    };
    const onVisibility = () => { if (document.visibilityState === "visible") fire(); };
    let day = localDateString(new Date());
    const interval = setInterval(() => {
      const now = localDateString(new Date());
      if (now !== day) { day = now; fire(); }
    }, DATE_CHECK_MS);
    window.addEventListener("focus", fire);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      if (timer !== null) clearTimeout(timer);
      clearInterval(interval);
      window.removeEventListener("focus", fire);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, []);
}
