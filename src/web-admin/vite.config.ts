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

// IMPORTANT: `base` must match the Rust admin router's static-file prefix.
// Currently the backend serves assets under "/admin/" (see src-tauri/src/admin/router.rs).
// When the backend migrates to root-level serving, change this to "/" and also update:
//   - index.html <base href="..."> and favicon path
//   - src-tauri/src/admin/static_files.rs admin_asset() path trim
//   - src-tauri/src/admin/router.rs static-file routes
const ADMIN_BASE = process.env.VITE_ADMIN_BASE_PATH ?? "/admin/";

export default defineConfig({
  root: __dirname,
  base: ADMIN_BASE,
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
