import type { HTMLAttributes, ReactNode } from "react";

type ScrollablePanelProps = HTMLAttributes<HTMLDivElement> & { children: ReactNode };

export function ScrollablePanel({ children, className = "", ...props }: ScrollablePanelProps) {
  return (
    <div className={className} {...props}>
      {children}
    </div>
  );
}
