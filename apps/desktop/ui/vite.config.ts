import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  // css: true keeps `?raw` CSS imports non-empty; the token and motion tests read the real stylesheets.
  test: { environment: "jsdom", globals: true, setupFiles: ["./src/test-setup.ts"], css: true },
});
