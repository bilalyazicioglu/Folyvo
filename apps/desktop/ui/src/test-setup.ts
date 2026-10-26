import "@testing-library/jest-dom/vitest";
import { clearMocks } from "@tauri-apps/api/mocks";
import { cleanup } from "@testing-library/react";

afterEach(async () => {
  cleanup();
  // Let listeners removed on unmount finish unregistering before the mocked internals go away.
  await new Promise((r) => setTimeout(r, 0));
  clearMocks();
});
