import "@testing-library/jest-dom/vitest";
import { clearMocks } from "@tauri-apps/api/mocks";
import { cleanup } from "@testing-library/react";

afterEach(() => {
  cleanup();
  clearMocks();
});
