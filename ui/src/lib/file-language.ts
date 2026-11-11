// The source view gets its language from the Rust API (files.rs language_for).
// A patch only carries paths, so the Git diff views need the same mapping on
// the client. Keep this table in sync with crates/pi-star-core/src/files.rs.

const EXTENSION_LANGUAGES: Record<string, string> = {
  ts: "typescript", mts: "typescript", cts: "typescript",
  tsx: "tsx",
  js: "javascript", mjs: "javascript", cjs: "javascript",
  jsx: "jsx",
  py: "python", rb: "ruby", php: "php", lua: "lua", pl: "perl", r: "r",
  go: "go", rs: "rust", java: "java",
  kt: "kotlin", kts: "kotlin",
  swift: "swift", scala: "scala", groovy: "groovy", gradle: "groovy",
  dart: "dart", zig: "zig",
  c: "c", h: "c", cpp: "cpp", cxx: "cpp", cc: "cpp", hpp: "cpp", hxx: "cpp",
  cs: "csharp", mm: "objectivec",
  ex: "elixir", exs: "elixir", hs: "haskell", ml: "ocaml", clj: "clojure", nix: "nix",
  html: "html", htm: "html", vue: "markup", svelte: "markup", astro: "markup",
  hbs: "handlebars", handlebars: "handlebars", mustache: "handlebars",
  css: "css", scss: "scss", less: "less",
  json: "json", jsonl: "json", jsonc: "json", json5: "json",
  yaml: "yaml", yml: "yaml", toml: "toml",
  ini: "ini", cfg: "ini", properties: "ini",
  xml: "xml", plist: "xml",
  md: "markdown", mdx: "markdown",
  tex: "latex",
  sh: "bash", bash: "bash", zsh: "bash", fish: "bash", env: "bash", gitignore: "bash",
  ps1: "powershell", psm1: "powershell",
  bat: "batch", cmd: "batch",
  sql: "sql", graphql: "graphql", gql: "graphql",
  proto: "protobuf", tf: "hcl", tfvars: "hcl", hcl: "hcl",
  diff: "diff", patch: "diff",
  http: "http",
  pdf: "pdf", docx: "word",
};

const NAME_LANGUAGES: Record<string, string> = {
  dockerfile: "dockerfile",
  containerfile: "dockerfile",
  makefile: "makefile",
  gnumakefile: "makefile",
  "cmakelists.txt": "cmake",
  "cargo.lock": "toml",
  ".gitignore": "bash",
  ".dockerignore": "bash",
  ".npmignore": "bash",
  ".eslintignore": "bash",
  ".prettierignore": "bash",
  ".bashrc": "bash",
  ".bash_profile": "bash",
  ".zshrc": "bash",
  ".profile": "bash",
  ".vimrc": "vim",
  ".gvimrc": "vim",
  ".editorconfig": "ini",
  ".npmrc": "ini",
  ".yarnrc": "ini",
  ".gitattributes": "ini",
  ".gitmodules": "ini",
  ".babelrc": "json",
  ".eslintrc": "json",
  ".prettierrc": "json",
  ".stylelintrc": "json",
  gemfile: "ruby",
  rakefile: "ruby",
  vagrantfile: "ruby",
  podfile: "ruby",
  brewfile: "ruby",
  fastfile: "ruby",
  guardfile: "ruby",
  justfile: "makefile",
  taskfile: "makefile",
};

function baseName(filePath: string): string {
  const parts = filePath.split(/[\\/]/);
  return (parts[parts.length - 1] ?? "").toLowerCase();
}

/** A Prism language id for a path, or "text" when the file has no grammar. */
export function languageForPath(filePath: string): string {
  const name = baseName(filePath);
  if (name === ".env" || name.startsWith(".env.")) return "bash";
  if (name === "dockerfile" || name.startsWith("dockerfile.")) return "dockerfile";
  if (name === "containerfile" || name.startsWith("containerfile.")) return "dockerfile";
  const byName = NAME_LANGUAGES[name];
  if (byName) return byName;
  const dot = name.lastIndexOf(".");
  const extension = dot === -1 ? "" : name.slice(dot + 1);
  return EXTENSION_LANGUAGES[extension] ?? "text";
}