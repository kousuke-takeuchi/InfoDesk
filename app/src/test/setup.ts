import "@testing-library/jest-dom/vitest";
import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";
import { clearMocks } from "@tauri-apps/api/mocks";

afterEach(async () => {
  cleanup();
  // アンマウント時の非同期な unlisten がモックを使い終えるのを待ってから片付ける
  await new Promise((r) => setTimeout(r, 0));
  clearMocks();
});
