import type { ReactNode } from "react";

type ContextPanelProps = {
  collapsed: boolean;
  title: string;
  subtitle?: string;
  children: ReactNode;
};

export function ContextPanel({ collapsed, title, subtitle, children }: ContextPanelProps) {
  return (
    <aside className="context-panel" aria-label={`${title}上下文`} aria-hidden={collapsed || undefined}>
      <header className="context-panel-header">
        <strong>{title}</strong>
        {subtitle && <span>{subtitle}</span>}
      </header>
      <div className="context-panel-body">{children}</div>
    </aside>
  );
}
