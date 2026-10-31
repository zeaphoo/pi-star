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
const minGit = arg("min-git", pins.minGit || "");
const minGitTag = arg("min-git-tag", pins.minGitTag || `v${minGit}.windows.1`);
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
if (platform === "win32") {
  required.push(join(outDir, "git/usr/bin/bash.exe"));
  required.push(join(outDir, "git/cmd/git.exe"));
}

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
  meta.arch === arch &&
  (platform === "win32" ? meta.git === minGit : !meta.git);
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

function extractZip(archive, dest) {
  mkdirSync(dest, { recursive: true });
  if (process.platform === "win32") {
    // Windows bsdtar handles zip archives (GNU tar on Linux does not).
    execFileSync("tar", ["-xf", archive, "-C", dest], { stdio: "inherit" });
  } else {
    execFileSync("unzip", ["-q", archive, "-d", dest], { stdio: "inherit" });
  }
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

function installMinGit(gitDir, work) {
  if (!minGit) throw new Error("packaging/pi-runtime.json is missing minGit");
  const bits = arch === "arm64" ? "arm64" : "64-bit";
  const name = `MinGit-${minGit}-${bits}.zip`;
  const url = `https://github.com/git-for-windows/git/releases/download/${minGitTag}/${name}`;
  const archive = join(work, name);
  mkdirSync(gitDir, { recursive: true });
  downloadSync(url, archive);
  extractZip(archive, gitDir);

  for (const relative of ["usr/bin/sh.exe", "cmd/git.exe", "LICENSE.txt"]) {
    if (!existsSync(join(gitDir, relative))) {
      throw new Error(`MinGit extraction is missing ${relative}`);
    }
  }
  // MinGit omits the bash.exe name but ships the identical bash binary as
  // sh.exe (verified byte-for-byte against PortableGit). Copy it so bash runs
  // in bash mode rather than POSIX sh mode.
  cpSync(join(gitDir, "usr/bin/sh.exe"), join(gitDir, "usr/bin/bash.exe"));

  cpSync(join(gitDir, "LICENSE.txt"), join(outDir, "GIT-LICENSE.txt"));
  writeFileSync(
    join(outDir, "GIT-NOTICE.txt"),
    `Git for Windows MinGit ${minGit}\n${url}\n\nGit is GPL-2.0; source is available from the URL above and https://github.com/git-for-windows/git.\n`,
  );
}

function downloadSync(url, dest) {
  console.log(`download ${url}`);
  execFileSync("curl", ["-fL", "--retry", "3", "--connect-timeout", "30", "-o", dest, url], {
    stdio: "inherit",
  });
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
    downloadSync(`https://nodejs.org/dist/v${nodeVersion}/${nodeArchiveName()}`, archive);
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

  // MinGit: Windows target machines may not have Git Bash. MinGit ships the
  // same bash binary as PortableGit under the name sh.exe, so copy it to
  // bash.exe; this keeps the bundle ~96 MB instead of ~416 MB.
  const gitDir = join(outDir, "git");
  rmSync(gitDir, { recursive: true, force: true });
  if (platform === "win32") {
    installMinGit(gitDir, work);
  }

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
    `${JSON.stringify(
      { node: nodeVersion, pi: piVersion, git: platform === "win32" ? minGit : null, platform, arch },
      null,
      2,
    )}\n`,
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
  if (platform === "win32") {
    const bash = join(outDir, "git/usr/bin/bash.exe");
    const git = join(outDir, "git/cmd/git.exe");
    const bashResult = spawnSync(bash, ["--noprofile", "--norc", "-c", "printf pi-star-git"], {
      encoding: "utf8",
    });
    if (bashResult.status !== 0 || bashResult.stdout.trim() !== "pi-star-git") {
      throw new Error(`bundled Git Bash failed: ${bashResult.stderr || bashResult.stdout}`);
    }
    const gitResult = spawnSync(git, ["--version"], { encoding: "utf8" });
    if (gitResult.status !== 0) {
      throw new Error(`bundled git failed: ${gitResult.stderr || gitResult.stdout}`);
    }
    console.log(`portable git ready: ${gitResult.stdout.trim()}`);
  }
  console.log(`pi runtime ready: node ${nodeVersion}, pi ${version}, ${platform}/${arch}`);
  console.log(`  ${outDir}`);
  rmSync(work, { recursive: true, force: true });
}

if (process.argv.includes("--git-only")) {
  if (platform !== "win32") {
    throw new Error("--git-only is only meaningful with --platform win32");
  }
  const work = join(root, "target/pi-runtime-work");
  rmSync(work, { recursive: true, force: true });
  mkdirSync(work, { recursive: true });
  mkdirSync(outDir, { recursive: true });
  installMinGit(join(outDir, "git"), work);
  rmSync(work, { recursive: true, force: true });
  console.log(`MinGit ready: ${join(outDir, "git")}`);
} else {
  await stage();
}
