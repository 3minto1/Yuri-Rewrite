export type AppEvent<T> = { payload: T };
export type DragDropPayload =
  | { type: "enter"; paths: string[] }
  | { type: "drop"; paths: string[] }
  | { type: "leave" }
  | { type: "over"; position: { x: number; y: number } };

export const browserMockEnabled =
  import.meta.env.DEV && import.meta.env.VITE_BROWSER_MOCK === "true";

function tauriAvailable() {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function listenAppEvent<T>(
  event: string,
  handler: (event: AppEvent<T>) => void
): Promise<() => void> {
  if (browserMockEnabled) return () => undefined;
  const { listen } = await import("@tauri-apps/api/event");
  return listen<T>(event, handler);
}

export async function listenDragDrop(
  handler: (event: AppEvent<DragDropPayload>) => void
): Promise<() => void> {
  if (browserMockEnabled || !tauriAvailable()) return () => undefined;
  const { getCurrentWebview } = await import("@tauri-apps/api/webview");
  return getCurrentWebview().onDragDropEvent((event) => {
    handler({ payload: event.payload as DragDropPayload });
  });
}

type DialogOptions = {
  directory?: boolean;
  multiple?: boolean;
  filters?: Array<{ name: string; extensions: string[] }>;
};

export async function openPathDialog(
  options: DialogOptions
): Promise<string | string[] | null> {
  if (browserMockEnabled || !tauriAvailable()) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  return open(options);
}
