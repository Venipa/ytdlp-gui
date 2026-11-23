// The app mark: the tray icon's down arrow on a Ledger tile.
export function Mark({ className }: { className?: string }) {
  return (
    <svg className={className} width="22" height="22" viewBox="0 0 22 22" aria-hidden focusable="false">
      <rect x="0.5" y="0.5" width="21" height="21" rx="2" fill="var(--l-drawer)" stroke="var(--l-edge)" />
      <path d="M11 5v9m-4-4 4 4 4-4M6.5 16.5h9" fill="none" stroke="currentColor" strokeWidth="1.6" />
    </svg>
  );
}
