import "@testing-library/jest-dom/vitest";
import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";
import { clearMocks } from "@tauri-apps/api/mocks";

// assistant-ui が使うが jsdom に無いブラウザ API の最小の代替 (何もしない)。
class ResizeObserverShim {
  observe() {}
  unobserve() {}
  disconnect() {}
}
if (!("ResizeObserver" in globalThis)) (globalThis as any).ResizeObserver = ResizeObserverShim;

if (!Element.prototype.scrollTo) Element.prototype.scrollTo = () => {};

afterEach(async () => {
  cleanup();
  // アンマウント時の非同期な unlisten がモックを使い終えるのを待ってから片付ける
  await new Promise((r) => setTimeout(r, 0));
  clearMocks();
});
