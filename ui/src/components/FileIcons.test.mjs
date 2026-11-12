import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import ts from "typescript";

const source = readFileSync(new URL("./FileIcons.tsx", import.meta.url), "utf8");
const { outputText } = ts.transpileModule(source, {
  compilerOptions: {
    target: ts.ScriptTarget.ES2022,
    module: ts.ModuleKind.CommonJS,
    jsx: ts.JsxEmit.React,
  },
});
const exports = {};
const React = {
  createElement: (tag, props, ...children) =>
    typeof tag === "function"
      ? tag({ ...(props ?? {}), children })
      : { tag, props: props ?? {}, children },
};
new Function("exports", "React", outputText)(exports, React);
const { getFileIcon, FolderIcon } = exports;

function iconName(node) {
  const value = node.props.style?.["--catppuccin-icon-light"];
  assert.equal(typeof value, "string", "every icon names an SVG");
  return value.match(/\/([^/]+)\.svg\)$/)?.[1];
}

test("common languages and their headers map to their own icons", () => {
  for (const [file, icon] of [
    ["main.rs", "rust"],
    ["Main.java", "java"],
    ["main.kt", "kotlin"],
    ["main.swift", "swift"],
    ["main.dart", "dart"],
    ["main.c", "c"],
    ["main.h", "c-header"],
    ["main.cpp", "cpp"],
    ["main.hpp", "cpp-header"],
    ["main.cs", "csharp"],
    ["index.php", "php"],
    ["app.rb", "ruby"],
    ["main.go", "go"],
    ["App.vue", "vue"],
    ["App.svelte", "svelte"],
    ["page.astro", "astro"],
    ["schema.proto", "proto"],
  ]) {
    assert.equal(iconName(getFileIcon(file)), icon, file);
  }
});

test("dotfiles, lock files and build/config names win over the extension", () => {
  for (const [file, icon] of [
    [".gitignore", "git"],
    [".editorconfig", "editorconfig"],
    ["Cargo.lock", "cargo-lock"],
    ["Cargo.toml", "cargo"],
    ["go.mod", "go-mod"],
    ["package.json", "package-json"],
    ["pnpm-lock.yaml", "pnpm-lock"],
    ["bun.lock", "bun-lock"],
    ["README.md", "readme"],
    ["LICENSE", "license"],
    ["CHANGELOG.md", "changelog"],
    ["tsconfig.json", "typescript-config"],
    ["vite.config.ts", "vite"],
    ["playwright.config.ts", "playwright"],
    ["Dockerfile.dev", "docker"],
    ["docker-compose.yml", "docker-compose"],
  ]) {
    assert.equal(iconName(getFileIcon(file)), icon, file);
  }
});

test("documents, media and archives get their own icons", () => {
  for (const [file, icon] of [
    ["report.pdf", "pdf"],
    ["report.docx", "ms-word"],
    ["sheet.xlsx", "ms-excel"],
    ["slides.pptx", "ms-powerpoint"],
    ["photo.png", "image"],
    ["logo.svg", "svg"],
    ["clip.mp4", "video"],
    ["sound.flac", "audio"],
    ["font.woff2", "font"],
    ["bundle.zip", "zip"],
    ["app.exe", "exe"],
    ["cert.pem", "certificate"],
    ["libfoo.so", "lib"],
  ]) {
    assert.equal(iconName(getFileIcon(file)), icon, file);
  }
});

test("the tree's folder names pick a folder icon, open or closed", () => {
  assert.equal(iconName(FolderIcon({ name: "src", open: false })), "folder_src");
  assert.equal(iconName(FolderIcon({ name: "src", open: true })), "folder_src_open");
  assert.equal(iconName(FolderIcon({ name: "node_modules", open: false })), "folder_node");
  assert.equal(iconName(FolderIcon({ name: "tests", open: true })), "folder_tests_open");
  assert.equal(iconName(FolderIcon({ name: "unknown", open: false })), "_folder");
  // A file with no known extension still gets the generic page icon.
  assert.equal(iconName(getFileIcon("LICENSE-WITHOUT-EXTENSION.txt")), "text");
  assert.equal(iconName(getFileIcon("mystery.bin")), "_file");
});