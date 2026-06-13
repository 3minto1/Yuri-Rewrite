import type { ReactNode } from "react";

type ModalProps = {
  children: ReactNode;
  className?: string;
  labelledBy?: string;
};

export function Modal({ children, className = "settings-dialog", labelledBy }: ModalProps) {
  return (
    <div className="modal-backdrop">
      <div className={className} role="dialog" aria-modal="true" aria-labelledby={labelledBy}>
        {children}
      </div>
    </div>
  );
}
