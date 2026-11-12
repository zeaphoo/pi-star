import assert from "node:assert/strict";
import test from "node:test";
import { createJiti } from "jiti";

const { resolveFileIcon, resolveFolderIcon, resolveGenericFileIcon } = await createJiti(import.meta.url).import("./icon-themes.ts");

const leaf = (url) => url.split("/").pop();

test("Catppuccin keeps its latte/mocha pair and its own name rules", () => {
  const rust = resolveFileIcon("catppuccin", "main.rs");
  assert.match(rust.light, /\/catppuccin\/latte\/rust\.svg$/);
  assert.match(rust.dark, /\/catppuccin\/mocha\/rust\.svg$/);
  assert.equal(leaf(resolveFileIcon("catppuccin", "Cargo.lock").light), "cargo-lock.svg");
  assert.equal(leaf(resolveFileIcon("catppuccin", "README.md").light), "readme.svg");
  assert.equal(leaf(resolveFolderIcon("catppuccin", "src", false).light), "folder_src.svg");
  assert.equal(leaf(resolveFolderIcon("catppuccin", "src", true).light), "folder_src_open.svg");
});

test("Material resolves file names, extensions and folders from its manifest", () => {
  assert.equal(leaf(resolveFileIcon("material", "main.rs").light), "rust.svg");
  assert.equal(leaf(resolveFileIcon("material", "Main.java").light), "java.svg");
  assert.equal(leaf(resolveFileIcon("material", "README.md").light), "readme.svg");
  assert.equal(leaf(resolveFileIcon("material", "package.json").light), "nodejs.svg");
  assert.equal(leaf(resolveFolderIcon("material", "src", false).light), "folder-src.svg");
  assert.equal(leaf(resolveFolderIcon("material", "src", true).light), "folder-src-open.svg");
  assert.equal(leaf(resolveGenericFileIcon("material").light), "file.svg");
});

test("VS Code Icons resolves language extensions and folder names", () => {
  assert.equal(leaf(resolveFileIcon("vscode", "main.rs").light), "file_type_rust.svg");
  assert.equal(leaf(resolveFileIcon("vscode", "Main.java").light), "file_type_java.svg");
  assert.equal(leaf(resolveFileIcon("vscode", "app.ts").light), "file_type_typescript.svg");
  assert.equal(leaf(resolveFileIcon("vscode", "README.md").light), "file_type_readme.svg");
  assert.equal(leaf(resolveFileIcon("vscode", "notes.md").light), "file_type_markdown.svg");
  assert.equal(leaf(resolveFolderIcon("vscode", "src", false).light), "folder_type_src.svg");
  assert.equal(leaf(resolveFolderIcon("vscode", "src", true).light), "folder_type_src_opened.svg");
  assert.equal(leaf(resolveGenericFileIcon("vscode").light), "default_file.svg");
});

test("Seti UI replays its suffix/substring rules and has light/dark ink", () => {
  assert.equal(leaf(resolveFileIcon("seti", "main.rs").light), "rust.svg");
  assert.equal(leaf(resolveFileIcon("seti", "Main.java").light), "java.svg");
  assert.equal(leaf(resolveFileIcon("seti", "app.ts").light), "typescript.svg");
  // The later filename rule wins over the generic .md suffix.
  assert.equal(leaf(resolveFileIcon("seti", "README.md").light), "info.svg");
  assert.match(resolveFileIcon("seti", "app.ts").light, /\/seti\/light\/typescript\.svg$/);
  assert.match(resolveFileIcon("seti", "app.ts").dark, /\/seti\/dark\/typescript\.svg$/);
  assert.equal(leaf(resolveFolderIcon("seti", "src", false).light), "folder.svg");
  assert.equal(leaf(resolveFolderIcon("seti", ".git", false).light), "git_folder.svg");
});

test("an unknown extension falls back to the theme's generic page icon", () => {
  for (const theme of ["catppuccin", "material", "vscode"]) {
    assert.equal(leaf(resolveFileIcon(theme, "mystery.zzz").light), leaf(resolveGenericFileIcon(theme).light));
  }
  // A folder that no theme names still gets a plain folder, open or closed.
  assert.equal(leaf(resolveFolderIcon("catppuccin", "nope", false).light), "_folder.svg");
  assert.equal(leaf(resolveFolderIcon("vscode", "nope", true).light), "default_folder_opened.svg");
});