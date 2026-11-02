/** 前後に移動できる期間の開始 (古い順)。保存済みのうち current 以前のものに、current を必ず足す (手で生成できるように)。 */
export function browseStarts(stored: string[], current: string): string[] {
  return [...new Set([...stored.filter((s) => s <= current), current])].sort();
}

/**
 * 表示する期間の開始。keep が移動できる開始ならそのまま。なければ current に要約があれば current、
 * なければ current より前の最新、それもなければ current。
 */
export function pickStart(stored: string[], current: string, keep: string | null): string {
  if (keep !== null && keep <= current && (keep === current || stored.includes(keep))) return keep;
  if (stored.includes(current)) return current;
  const earlier = stored.filter((s) => s < current).sort();
  return earlier.length > 0 ? earlier[earlier.length - 1] : current;
}
