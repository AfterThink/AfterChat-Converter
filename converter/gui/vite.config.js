import { defineConfig } from "vite";

const host = "127.0.0.1";
const port = 1420;

export default defineConfig({
  clearScreen: false,
  server: {
    host,
    port,
    strictPort: true,
  },
  preview: {
    host,
    port,
    strictPort: true,
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: process.env.TAURI_PLATFORM === "windows" ? "chrome105" : "safari13",
    minify: process.env.TAURI_DEBUG ? false : "esbuild",
    sourcemap: Boolean(process.env.TAURI_DEBUG),
  },
});
