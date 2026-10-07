export function Toggle({ checked, onChange, ariaLabel }: { checked: boolean; onChange: (v: boolean) => void; ariaLabel: string }) {
  return <button className="toggle" role="switch" aria-checked={checked} aria-label={ariaLabel} onClick={() => onChange(!checked)} />;
}
