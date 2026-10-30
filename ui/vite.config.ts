import path from "node:path";
import { fileURLToPath } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const root = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  define: {
    "import.meta.env.VITE_APP_VERSION": JSON.stringify(process.env.PI_STAR_VERSION ?? "0.1.0"),
    "import.meta.env.VITE_PI_VERSION": JSON.stringify("pi-compatible-rust"),
  },
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(root, "src"),
      "next/navigation": path.resolve(root, "src/shims/next-navigation.ts"),
      "next/image": path.resolve(root, "src/shims/next-image.tsx"),
      "@earendil-works/pi-ai/utils/json-parse": path.resolve(root, "src/shims/pi-json-parse.ts"),
    },
  },
  build: {
    outDir: "dist",
    sourcemap: false,
    chunkSizeWarningLimit: 2000,
  },
  server: {
    proxy: {
      "/api": "http://127.0.0.1:30151",
    },
  },
});
