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
      // The package's "browser" build decodes entities with
      // document.createElement at module scope, which throws in the highlight
      // worker. Its worker condition is the DOM-free implementation.
      "decode-named-character-reference": path.resolve(
        root,
        "node_modules/decode-named-character-reference/index.js",
      ),
    },
  },
  build: {
    outDir: "dist",
    sourcemap: false,
    chunkSizeWarningLimit: 2000,
  },
  server: {
    proxy: {
      // A string target makes Vite set changeOrigin: true, which rewrites Host
      // to 30151 while the page's Origin stays 30152 — the server's same-origin
      // check then rejects every POST with "Untrusted origin". Keep the page's
      // Host so Host and Origin match, as they do when the UI is served by
      // pi-star itself.
      "/api": {
        target: "http://127.0.0.1:30151",
        changeOrigin: false,
      },
    },
  },
});
