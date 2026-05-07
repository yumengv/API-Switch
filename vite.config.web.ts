import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "path";

export default defineConfig({
  root: path.resolve(__dirname, "src"),
  base: "/",
  publicDir: path.resolve(__dirname, "public"),
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "src"),
      "@tauri-apps/api/core": path.resolve(__dirname, "src/stubs/tauri-core.ts"),
      "@tauri-apps/api/app": path.resolve(__dirname, "src/stubs/tauri-app.ts"),
      "@tauri-apps/plugin-opener": path.resolve(__dirname, "src/stubs/tauri-opener.ts"),
    },
  },
  build: {
    outDir: path.resolve(__dirname, "dist-web-admin"),
    emptyOutDir: true,
    assetsDir: "assets",
    rollupOptions: {
      input: path.resolve(__dirname, "src/index.html"),
      output: {
        entryFileNames: "assets/[name]-[hash].js",
        chunkFileNames: "assets/[name]-[hash].js",
        assetFileNames: "assets/[name]-[hash].[ext]",
      },
    },
  },
});
