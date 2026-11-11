import assert from "node:assert/strict";
import test from "node:test";
import { createJiti } from "jiti";

const { languageForPath } = await createJiti(import.meta.url).import("./file-language.ts");

test("extensions map to the Prism grammar the source view uses", () => {
  assert.equal(languageForPath("src/main.rs"), "rust");
  assert.equal(languageForPath("src/App.tsx"), "tsx");
  assert.equal(languageForPath("web/src/app.mjs"), "javascript");
  assert.equal(languageForPath("styles/theme.scss"), "scss");
  assert.equal(languageForPath("proto/service.proto"), "protobuf");
  assert.equal(languageForPath("deploy/main.tfvars"), "hcl");
  assert.equal(languageForPath("head.hbs"), "handlebars");
});

test("dotfiles and lock files keep the language their name implies", () => {
  // A dotfile has no extension, so matching on one alone leaves the most
  // common config files plain.
  assert.equal(languageForPath(".gitignore"), "bash");
  assert.equal(languageForPath("nested/.editorconfig"), "ini");
  assert.equal(languageForPath("Cargo.lock"), "toml");
  assert.equal(languageForPath("Info.plist"), "xml");
  assert.equal(languageForPath("CMakeLists.txt"), "cmake");
  assert.equal(languageForPath("Gemfile"), "ruby");
  assert.equal(languageForPath("Dockerfile.dev"), "dockerfile");
  assert.equal(languageForPath(".env.local"), "bash");
});

test("an unknown path stays plain", () => {
  assert.equal(languageForPath("LICENSE"), "text");
  assert.equal(languageForPath("app/icons/icon.png"), "text");
  // The diff views pass an empty path for a patch with no file header.
  assert.equal(languageForPath(""), "text");
});
