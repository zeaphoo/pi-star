#!/usr/bin/env node
// Copy the reachable client-side module graph from the read-only pi-web checkout.
// Server-only modules (API routes, Node imports) are intentionally not copied;
// their behaviour lives in crates/pi-star-core and crates/pi-star.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, "..");
const piWeb = process.env.PI_WEB_DIR
  ? path.resolve(process.env.PI_WEB_DIR)
  : path.resolve(repo, "../pi-web");
const out = path.join(repo, "ui", "src");

const entries = ["app/page.tsx", "app/login/page.tsx", "components/AppShell.tsx", "components/PwaRegistration.tsx"];
const extraCss = [
  "app/globals.css",
  "app/settings.css",
  "app/sidebar.css",
  "app/sidebar-menu.css",
];
const extensions = ["", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".css", ".json"];
const seen = new Set();
// Server-only modules reachable only through `import type` in the copied
// client graph. Their behaviour lives in Rust; keeping them here would pull
// Node built-ins into the frontend typecheck.
const excluded = new Set([
  "app/layout.tsx",
  "lib/allowed-roots.ts",
  "lib/atomic-file.ts",
  "lib/file-tree-visibility.ts",
  "lib/path-security.ts",
  "lib/paths.ts",
  "lib/session-search.ts",
  "lib/subagent-extension.ts",
  "lib/subagent-input.ts",
  "lib/subagent-settings.ts",
  "lib/subagents.ts",
  "lib/terminal-manager.ts",
  "lib/worktree.ts",
]);

function resolveSpec(from, spec) {
  if (spec.startsWith("@/")) return path.join(piWeb, spec.slice(2));
  if (spec.startsWith(".")) return path.resolve(path.dirname(path.join(piWeb, from)), spec);
  return null;
}

function findFile(base) {
  for (const ext of extensions) {
    const candidate = base + ext;
    if (fs.existsSync(candidate) && fs.statSync(candidate).isFile()) return candidate;
  }
  for (const ext of [".ts", ".tsx", ".js", ".jsx"]) {
    const candidate = path.join(base, "index" + ext);
    if (fs.existsSync(candidate)) return candidate;
  }
  return null;
}

function walk(rel) {
  if (seen.has(rel)) return;
  const abs = path.join(piWeb, rel);
  if (!fs.existsSync(abs)) throw new Error(`missing ${rel}`);
  seen.add(rel);
  if (!/\.(tsx?|jsx?|mjs|css)$/.test(abs)) return;
  const text = fs.readFileSync(abs, "utf8");
  for (const match of text.matchAll(/(?:from\s+|import\s*\(\s*|require\s*\(\s*)["']([^"']+)["']/g)) {
    const spec = match[1].split("?")[0];
    if (spec.startsWith("next/") || spec.startsWith("@earendil-works/")) continue;
    if (/^(react|react-dom|js-yaml|semver|proper-lockfile|undici|web-push|node-pty|katex|mermaid|mammoth|ansi_up|react-markdown|remark-|rehype-|react-syntax-highlighter|@xterm\/)/.test(spec)) continue;
    const base = resolveSpec(rel, spec);
    if (!base) continue;
    const file = findFile(base);
    if (!file) throw new Error(`${rel}: cannot resolve ${spec}`);
    walk(path.relative(piWeb, file));
  }
}

for (const entry of entries) walk(entry);
for (const css of extraCss) {
  if (!seen.has(css)) walk(css);
}

// Local host adaptations applied after copying. Keeping them here means a
// re-sync from upstream does not silently drop the Vite/React changes.
function postProcess(dest, rel) {
  let text = fs.readFileSync(dest, "utf8");
  const replace = (from, to) => { text = text.split(from).join(to); };
  if (rel === "components/PwaRegistration.tsx") {
    replace('process.env.NODE_ENV !== "production"', '!import.meta.env.PROD');
    replace('process.env.NEXT_PUBLIC_APP_VERSION', 'import.meta.env.VITE_APP_VERSION');
  }
  if (rel === "components/PiWebTitle.tsx" || rel === "components/ChatWindow.tsx") {
    replace('process.env.NEXT_PUBLIC_APP_VERSION', 'import.meta.env.VITE_APP_VERSION');
    replace('process.env.NEXT_PUBLIC_PI_VERSION', 'import.meta.env.VITE_PI_VERSION');
  }
  if (rel === "lib/i18n/format.ts") {
    replace('process.env.NODE_ENV !== "production"', 'import.meta.env?.DEV');
  }
  if (rel === "app/globals.css") {
    replace(`/* demo/ is a separate project; keep its sources out of this app's class scan. */\n@source not "../demo";\n/* Agent worktrees: scanning them makes webpack snapshot their symlinked\n   node_modules, which crashes \`next build\` with "reading 'length'". */\n@source not "../.claude";\n`, "");
    replace("var(--font-noto-mono), 'JetBrains Mono'", "'Noto Sans Mono', 'JetBrains Mono'");
  }
  if (rel === "components/DelimitedTable.tsx") {
    replace('import { useEffect, useMemo, useState, type CSSProperties, type RefObject } from "react";', 'import { useEffect, useMemo, useState, type CSSProperties, type ReactNode, type RefObject } from "react";');
    replace("  const rows = [];", "  const rows: ReactNode[] = [];");
  }
  if (rel === "components/MessageView.tsx") {
    replace("toolFieldsOfInput(isStreamingInput ? parseStreamingJson(block.rawInput) : block.input)", 'toolFieldsOfInput(isStreamingInput ? parseStreamingJson(block.rawInput ?? "") : block.input)');
    replace("  const parts = [];\n  if (readFiles.length > 0)", "  const parts: string[] = [];\n  if (readFiles.length > 0)");
    replace("  const parts = [];\n  if (usage.input)", "  const parts: string[] = [];\n  if (usage.input)");
  }
  if (rel === "components/PluginsConfig.tsx") {
    replace("  const parts = [];\n  if (pkg.version)", "  const parts: string[] = [];\n  if (pkg.version)");
  }
  if (rel === "components/AgentsConfig.tsx") {
    replace("onChange={setTargetScope}", "onChange={(value) => setTargetScope(value as SubagentWritableScope)}");
  }
  if (rel === "components/SkillsConfig.tsx") {
    replace("onChange={setScope}", 'onChange={(value) => setScope(value as "project" | "global")}');
  }
  if (rel === "lib/mcp-import.ts") {
    replace("configValueEnvVarNames(value);", "configValueEnvVarNames(String(value));");
    replace("configValueEnvVarNames(config.oauth?.clientSecret ?? \"\")", "configValueEnvVarNames(String(config.oauth?.clientSecret ?? \"\"))");
  }
  fs.writeFileSync(dest, text);
}

for (const dir of ["app", "components", "hooks", "lib"]) {
  fs.rmSync(path.join(out, dir), { recursive: true, force: true });
}
for (const rel of [...seen].sort()) {
  const src = path.join(piWeb, rel);
  const dest = path.join(out, rel);
  fs.mkdirSync(path.dirname(dest), { recursive: true });
  fs.copyFileSync(src, dest);
  postProcess(dest, rel);
}

const publicDir = path.join(repo, "ui", "public");
fs.rmSync(publicDir, { recursive: true, force: true });
fs.cpSync(path.join(piWeb, "public"), publicDir, { recursive: true });

fs.writeFileSync(
  path.join(repo, "ui", "UPSTREAM.md"),
  `# UI upstream\n\nCopied from the read-only pi-web checkout at \`${piWeb}\`.\n\n` +
  `- Source project: https://github.com/agegr/pi-web (MIT)\n` +
  `- Copied client modules: ${seen.size}\n` +
  `- Re-copy: \`node scripts/sync-ui-from-pi-web.mjs\`\n` +
  `- Do not copy \`app/api/**\` or server-only \`lib/**\` modules; those are implemented in Rust.\n`,
);
console.log(`copied ${seen.size} modules into ui/src and public into ui/public`);
