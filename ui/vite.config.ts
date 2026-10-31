import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const root = path.dirname(fileURLToPath(import.meta.url));

// The desktop installer pins the bundled pi version. Keep the UI's displayed
// version in sync with that pin instead of a hard-coded compatibility string.
function packageVersion() {
  try {
    return JSON.parse(readFileSync(path.resolve(root, "package.json"), "utf8")).version || "0.0.0";
  } catch {
    return "0.0.0";
  }
}

function bundledPiVersion() {
  try {
    const pins = JSON.parse(readFileSync(path.resolve(root, "../packaging/pi-runtime.json"), "utf8"));
    return typeof pins.pi === "string" && pins.pi ? pins.pi : "0.0.0";
  } catch {
    return "0.0.0";
  }
}

export default defineConfig({
  define: {
    "import.meta.env.VITE_APP_VERSION": JSON.stringify(process.env.PI_STAR_VERSION ?? packageVersion()),
    "import.meta.env.VITE_PI_VERSION": JSON.stringify(process.env.PI_STAR_PI_VERSION ?? bundledPiVersion()),
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
