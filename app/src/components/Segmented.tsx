import type { View } from "./Sidebar";

export function Segmented<T extends string>({ options, value, onChange, ariaLabel }: {
  options: { id: T; label: string }[]; value: T; onChange: (v: T) => void; ariaLabel?: string;
}) {
  return (
    <div className="segmented" role="group" aria-label={ariaLabel}>
      {options.map((o) => (
        <button key={o.id} aria-pressed={o.id === value} onClick={() => onChange(o.id)}>{o.label}</button>
      ))}
    </div>
  );
}

const VIEW_OPTIONS: { id: Exclude<View, "settings">; label: string }[] = [
  { id: "digest", label: "ダイジェスト" },
  { id: "feed", label: "フィード" },
];

export function ViewSwitch({ view, onChange }: { view: View; onChange: (v: View) => void }) {
  return <Segmented options={VIEW_OPTIONS} value={view === "settings" ? "digest" : view} onChange={onChange} ariaLabel="表示" />;
}
