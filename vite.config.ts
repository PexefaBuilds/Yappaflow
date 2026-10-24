import { defineConfig } from "vite";

// Tauri expects a fixed dev port and does its own console output.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: "es2021",
    outDir: "dist",
    rollupOptions: { input: { main: "index.html", pill: "pill.html", onboarding: "onboarding.html", toast: "toast.html", tray: "tray.html" } },
  },
});
