import "@testing-library/jest-dom/vitest";

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}

Object.defineProperty(globalThis, "ResizeObserver", { value: ResizeObserverMock, writable: true });
if (typeof Element !== "undefined") {
  Object.defineProperty(Element.prototype, "scrollIntoView", { value() {}, writable: true });
}
