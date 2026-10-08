export function Toggle({ checked, onChange, ariaLabel, disabled }: { checked: boolean; onChange: (v: boolean) => void; ariaLabel: string; disabled?: boolean }) {
  return <button className="toggle" role="switch" aria-checked={checked} aria-label={ariaLabel} disabled={disabled} onClick={() => onChange(!checked)} />;
}
