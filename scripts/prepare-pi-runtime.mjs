#!/usr/bin/env node
/**
 * Stage the bundled `pi` runtime used by desktop installers.
 *
 * Layout:
 *   app/pi-runtime/
 *     node(.exe)             official Node.js runtime
 *     pi/                    @earendil-works/pi-coding-agent + prod deps
 *     agent-extension/       pi-star's host extension (index.ts, subagent.ts)
 *     pi-runtime.json        pinned versions / target
 *     NODE-LICENSE, PI-NOTICE
 *
 * The result is gitignored and consumed by scripts/package-*.sh through
 * Tauri's `bundle.resources` injection.
 */

import { execFileSync, spawnSync } from "node:child_process";
import {
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const pins = JSON.parse(readFileSync(join(root, "packaging/pi-runtime.json"), "utf8"));

function arg(name, fallback = "") {
  const index = process.argv.indexOf(`--${name}`);
  return index >= 0 ? process.argv[index + 1] : fallback;
}

const outDir = resolve(arg("out", join(root, "app/pi-runtime")));
const platform = arg("platform", process.platform);
const arch = arg("arch", process.arch);
const force = process.argv.includes("--force");
const nodeVersion = arg("node-version", pins.node);
const piVersion = arg("pi-version", pins.pi);
const localNode = arg("node", process.env.PI_RUNTIME_NODE || "");
const localPiDir = arg("pi-dir", process.env.PI_RUNTIME_PI_DIR || "");

if (!["linux", "darwin", "win32"].includes(platform)) {
  throw new Error(`unsupported platform: ${platform}`);
}
if (!["x64", "arm64"].includes(arch)) {
  throw new Error(`unsupported arch: ${arch}`);
}

const metaPath = join(outDir, "pi-runtime.json");
const required = [
  join(outDir, platform === "win32" ? "node.exe" : "node"),
  join(outDir, "pi/package.json"),
  join(outDir, "pi/dist/bundle/cli.js"),
  join(outDir, "agent-extension/index.ts"),
];

function currentMeta() {
  try {
    return JSON.parse(readFileSync(metaPath, "utf8"));
  } catch {
    return null;
  }
}

const meta = currentMeta();
const fresh =
  meta &&
  meta.node === nodeVersion &&
  meta.pi === piVersion &&
  meta.platform === platform &&
  meta.arch === arch;
if (!force && fresh && required.every(existsSync)) {
  console.log(`pi runtime already prepared: ${outDir} (node ${nodeVersion}, pi ${piVersion}, ${platform}/${arch})`);
  process.exit(0);
}

function npmCommand() {
  return platform === "win32" ? "npm.cmd" : "npm";
}

function runNpm(args, cwd) {
  const result = spawnSync(npmCommand(), args, {
    cwd,
    stdio: "inherit",
    shell: platform === "win32",
    env: process.env,
  });
  if (result.status !== 0) {
    throw new Error(`npm ${args.join(" ")} failed with status ${result.status}`);
  }
}

function extract(archive, dest) {
  mkdirSync(dest, { recursive: true });
  execFileSync("tar", ["-xf", archive, "-C", dest], { stdio: "inherit" });
}

async function download(url, dest) {
  console.log(`download ${url}`);
  // curl is available on GitHub's Linux/macOS/Windows runners and avoids
  // Node's undici IPv4/IPv6 connect timeout on some CI networks.
  execFileSync("curl", ["-fL", "--retry", "3", "--connect-timeout", "30", "-o", dest, url], {
    stdio: "inherit",
  });
}

function findFile(dir, name) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isFile() && entry.name === name) return path;
    if (entry.isDirectory()) {
      const found = findFile(path, name);
      if (found) return found;
    }
  }
  return null;
}

function nodeArchiveName() {
  const suffix =
    platform === "win32"
      ? `win-${arch}.zip`
      : platform === "darwin"
        ? `darwin-${arch}.tar.gz`
        : `linux-${arch}.tar.xz`;
  return `node-v${nodeVersion}-${suffix}`;
}

async function stage() {
  const work = join(root, "target/pi-runtime-work");
  rmSync(work, { recursive: true, force: true });
  mkdirSync(work, { recursive: true });
  mkdirSync(outDir, { recursive: true });

  // Node runtime
  if (localNode) {
    const target = join(outDir, platform === "win32" ? "node.exe" : "node");
    cpSync(localNode, target);
    console.log(`node runtime copied from ${localNode}`);
  } else {
    const archive = join(work, nodeArchiveName());
    await download(`https://nodejs.org/dist/v${nodeVersion}/${nodeArchiveName()}`, archive);
    const extracted = join(work, "node");
    extract(archive, extracted);
    const nodeName = platform === "win32" ? "node.exe" : "node";
    const found = findFile(extracted, nodeName);
    if (!found) throw new Error(`${nodeName} not found in ${nodeArchiveName()}`);
    cpSync(found, join(outDir, nodeName));
    const license = findFile(extracted, "LICENSE");
    if (license) cpSync(license, join(outDir, "NODE-LICENSE"));
  }

  // pi package
  rmSync(join(outDir, "pi"), { recursive: true, force: true });
  if (localPiDir) {
    cpSync(localPiDir, join(outDir, "pi"), { recursive: true });
    console.log(`pi package copied from ${localPiDir}`);
  } else {
    const packed = join(work, "packed");
    mkdirSync(packed, { recursive: true });
    runNpm(["pack", `@earendil-works/pi-coding-agent@${piVersion}`, "--pack-destination", packed], work);
    const tarball = readdirSync(packed).find((name) => name.endsWith(".tgz"));
    if (!tarball) throw new Error("npm pack did not produce a .tgz");
    const extracted = join(work, "pi");
    extract(join(packed, tarball), extracted);
    cpSync(join(extracted, "package"), join(outDir, "pi"), { recursive: true });
    // The published pi tarball ships an npm-shrinkwrap that can lag its own
    // dependencies (0.99.1 shrinkwrap referenced 0.99.2 in practice), so `npm ci`
    // rejects it. Resolve production deps from package.json instead.
    runNpm(
      ["install", "--omit=dev", "--ignore-scripts", "--no-audit", "--no-fund", "--no-package-lock"],
      join(outDir, "pi"),
    );
  }

  // host extension (TypeScript source is loaded by pi's extension loader)
  rmSync(join(outDir, "agent-extension"), { recursive: true, force: true });
  cpSync(join(root, "agent-extension"), join(outDir, "agent-extension"), { recursive: true });

  const piLicense = ["LICENSE", "LICENSE.md", "NOTICE"]
    .map((name) => join(outDir, "pi", name))
    .find(existsSync);
  if (piLicense) cpSync(piLicense, join(outDir, "PI-LICENSE"));
  writeFileSync(
    join(outDir, "PI-NOTICE"),
    `@earendil-works/pi-coding-agent ${piVersion}\nhttps://www.npmjs.com/package/@earendil-works/pi-coding-agent\n\nNode.js ${nodeVersion}\nhttps://nodejs.org/\n`,
  );

  writeFileSync(
    metaPath,
    `${JSON.stringify({ node: nodeVersion, pi: piVersion, platform, arch }, null, 2)}\n`,
  );

  // Smoke-test the bundled pair. PI_CODING_AGENT_DIR keeps it away from user config.
  const node = join(outDir, platform === "win32" ? "node.exe" : "node");
  const cli = join(outDir, "pi/dist/bundle/cli.js");
  const agentDir = join(work, "agent");
  mkdirSync(agentDir, { recursive: true });
  const result = spawnSync(node, [cli, "--version"], {
    env: { ...process.env, PI_CODING_AGENT_DIR: agentDir },
    encoding: "utf8",
  });
  if (result.status !== 0) {
    throw new Error(`bundled pi failed to start: ${result.stderr || result.stdout}`);
  }
  const version = (result.stdout || "").trim().split(/\s+/).pop();
  console.log(`pi runtime ready: node ${nodeVersion}, pi ${version}, ${platform}/${arch}`);
  console.log(`  ${outDir}`);
  rmSync(work, { recursive: true, force: true });
}

await stage();
