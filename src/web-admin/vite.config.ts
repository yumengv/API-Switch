import { copyFileSync, existsSync } from "fs";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "path";

const publicDir = path.resolve(__dirname, "../../public");
const outDir = path.resolve(__dirname, "../../dist-web-admin");
const faviconSource = path.resolve(__dirname, "../../src-tauri/icons/icon.ico");

function adminFaviconPlugin() {
  return {
    name: "admin-favicon",
    closeBundle() {
      if (existsSync(faviconSource)) {
        copyFileSync(faviconSource, path.join(outDir, "favicon.ico"));
      }
    },
  };
}

export default defineConfig({
  root: __dirname,
  base: "/admin/",
  publicDir,
  plugins: [react(), tailwindcss(), adminFaviconPlugin()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "../"),
    },
  },
  build: {
    outDir,
    emptyOutDir: true,
    assetsDir: "assets",
    manifest: true,
    rollupOptions: {
      output: {
        entryFileNames: "assets/[name]-[hash].js",
        chunkFileNames: "assets/[name]-[hash].js",
        assetFileNames: "assets/[name]-[hash].[ext]",
      },
    },
  },
});
