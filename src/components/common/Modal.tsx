import { createPortal } from "react-dom";
import { useEffect, useRef, type ReactNode, type RefObject } from "react";

type ModalProps = {
  children: ReactNode;
  className?: string;
  labelledBy?: string;
  onRequestClose?: () => void;
  initialFocusRef?: RefObject<HTMLElement | null>;
};

type ModalLayer = {
  backdrop: HTMLDivElement;
  dialog: HTMLDivElement;
};

const modalStack: ModalLayer[] = [];
let rootAccessibilityState: { inert: boolean; ariaHidden: string | null } | null = null;

function syncModalLayers() {
  const appRoot = document.getElementById("root");
  const topLayer = modalStack[modalStack.length - 1];
  if (appRoot && modalStack.length > 0) {
    if (!rootAccessibilityState) {
      rootAccessibilityState = {
        inert: appRoot.hasAttribute("inert"),
        ariaHidden: appRoot.getAttribute("aria-hidden")
      };
    }
    appRoot.setAttribute("inert", "");
    appRoot.setAttribute("aria-hidden", "true");
  } else if (appRoot && rootAccessibilityState) {
    if (!rootAccessibilityState.inert) appRoot.removeAttribute("inert");
    if (rootAccessibilityState.ariaHidden === null) appRoot.removeAttribute("aria-hidden");
    else appRoot.setAttribute("aria-hidden", rootAccessibilityState.ariaHidden);
    rootAccessibilityState = null;
  }
  for (const layer of modalStack) {
    if (layer === topLayer) {
      layer.backdrop.removeAttribute("inert");
      layer.backdrop.removeAttribute("aria-hidden");
    } else {
      layer.backdrop.setAttribute("inert", "");
      layer.backdrop.setAttribute("aria-hidden", "true");
    }
  }
}

function focusableElements(dialog: HTMLElement) {
  return Array.from(dialog.querySelectorAll<HTMLElement>(
    "button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), a[href], [tabindex]:not([tabindex='-1'])"
  )).filter((element) => !element.hasAttribute("hidden") && element.getAttribute("aria-hidden") !== "true");
}

export function Modal({
  children,
  className = "settings-dialog",
  labelledBy,
  onRequestClose,
  initialFocusRef
}: ModalProps) {
  const backdropRef = useRef<HTMLDivElement>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const closeHandlerRef = useRef(onRequestClose);
  closeHandlerRef.current = onRequestClose;

  useEffect(() => {
    const backdrop = backdropRef.current;
    const dialog = dialogRef.current;
    if (!backdrop || !dialog) return undefined;
    const backdropElement: HTMLDivElement = backdrop;
    const dialogElement: HTMLDivElement = dialog;
    const previouslyFocused = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const layer = { backdrop: backdropElement, dialog: dialogElement };
    modalStack.push(layer);
    syncModalLayers();

    const focusFrame = window.requestAnimationFrame(() => {
      const requested = initialFocusRef?.current;
      const firstFocusable = focusableElements(dialogElement)[0];
      (requested && dialogElement.contains(requested) ? requested : firstFocusable ?? dialogElement).focus();
    });

    function handleKeyDown(event: KeyboardEvent) {
      if (modalStack[modalStack.length - 1] !== layer) return;
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopImmediatePropagation();
        closeHandlerRef.current?.();
        return;
      }
      if (event.key !== "Tab") return;
      const focusable = focusableElements(dialogElement);
      if (focusable.length === 0) {
        event.preventDefault();
        dialogElement.focus();
        return;
      }
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      const active = document.activeElement;
      if (event.shiftKey && (active === first || !dialogElement.contains(active))) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && (active === last || !dialogElement.contains(active))) {
        event.preventDefault();
        first.focus();
      }
    }

    document.addEventListener("keydown", handleKeyDown, true);
    return () => {
      window.cancelAnimationFrame(focusFrame);
      document.removeEventListener("keydown", handleKeyDown, true);
      const wasTopLayer = modalStack[modalStack.length - 1] === layer;
      const layerIndex = modalStack.indexOf(layer);
      if (layerIndex >= 0) modalStack.splice(layerIndex, 1);
      syncModalLayers();
      if (wasTopLayer) {
        const nextLayer = modalStack[modalStack.length - 1];
        if (nextLayer) {
          (focusableElements(nextLayer.dialog)[0] ?? nextLayer.dialog).focus();
        } else if (previouslyFocused?.isConnected) {
          previouslyFocused.focus();
        }
      }
    };
  }, [initialFocusRef]);

  return createPortal(
    <div className="modal-backdrop" ref={backdropRef}>
      <div
        className={className}
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={labelledBy}
        tabIndex={-1}
      >
        {children}
      </div>
    </div>,
    document.body
  );
}
