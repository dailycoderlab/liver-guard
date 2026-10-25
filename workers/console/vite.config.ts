import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  // Vite resolves `root` from cwd, but this config lives in console/ and is
  // invoked from workers/, so pin it to the config file's own directory.
  root: import.meta.dirname,
  plugins: [react()],
  base: "./",
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "es2022",
  },
  server: {
    port: 5173,
    proxy: {
      "/v1": { target: "http://127.0.0.1:8787", changeOrigin: true, ws: true },
    },
  },
});
