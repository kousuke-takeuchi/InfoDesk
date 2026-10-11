export function Sparkline({ values, ariaLabel }: { values: number[]; ariaLabel: string }) {
  const W = 96, H = 20, P = 2;
  if (values.length === 0) return <svg width={W} height={H} role="img" aria-label={ariaLabel} />;
  const top = Math.max(0, ...values);
  const step = values.length > 1 ? (W - P * 2) / (values.length - 1) : 0;
  const points = values.map((v, i) => {
    const x = P + i * step;
    const y = top > 0 ? H - P - (v / top) * (H - P * 2) : H - P;
    return `${x.toFixed(1)},${y.toFixed(1)}`;
  }).join(" ");
  return (
    <svg width={W} height={H} role="img" aria-label={ariaLabel} className="sparkline">
      <polyline points={points} fill="none" stroke="currentColor" strokeWidth={1.5} />
    </svg>
  );
}
