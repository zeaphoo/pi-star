import type { CSSProperties } from "react";


interface IconProps {
  size?: number;
}

type FileIconName =
  | "_file"
  | "_folder"
  | "_folder_open"
  | "angular"
  | "apache"
  | "assembly"
  | "astro"
  | "audio"
  | "babel"
  | "bash"
  | "bazel"
  | "bitbucket"
  | "bun"
  | "bun-lock"
  | "c"
  | "c-header"
  | "caddy"
  | "cargo"
  | "cargo-lock"
  | "certificate"
  | "changelog"
  | "clojure"
  | "cmake"
  | "cobol"
  | "code-of-conduct"
  | "codeowners"
  | "config"
  | "contributing"
  | "cpp"
  | "cpp-header"
  | "crystal"
  | "csharp"
  | "css"
  | "csv"
  | "cypress"
  | "dart"
  | "database"
  | "deno"
  | "deno_lock"
  | "docker"
  | "docker-compose"
  | "docker-ignore"
  | "editorconfig"
  | "elixir"
  | "env"
  | "envrc"
  | "erlang"
  | "esbuild"
  | "eslint"
  | "exe"
  | "firebase"
  | "font"
  | "git"
  | "gitlab"
  | "go"
  | "go-mod"
  | "gradle"
  | "graphql"
  | "groovy"
  | "haskell"
  | "html"
  | "image"
  | "java"
  | "javascript"
  | "javascript-config"
  | "javascript-react"
  | "jest"
  | "json"
  | "json-schema"
  | "julia"
  | "just"
  | "key"
  | "kotlin"
  | "less"
  | "lib"
  | "license"
  | "lock"
  | "log"
  | "lua"
  | "makefile"
  | "markdown"
  | "markdown-mdx"
  | "matlab"
  | "meson"
  | "ms-excel"
  | "ms-powerpoint"
  | "ms-word"
  | "netlify"
  | "next"
  | "nginx"
  | "ninja"
  | "nix"
  | "nix-lock"
  | "npm"
  | "npm-lock"
  | "nuget"
  | "ocaml"
  | "package-json"
  | "pdf"
  | "perl"
  | "php"
  | "playwright"
  | "pnpm"
  | "pnpm-lock"
  | "poetry-lock"
  | "postcss"
  | "powershell"
  | "prettier"
  | "prisma"
  | "proto"
  | "python"
  | "python-compiled"
  | "python-config"
  | "r"
  | "readme"
  | "rollup"
  | "ruby"
  | "ruby-gem"
  | "ruby-gem-lock"
  | "rust"
  | "sass"
  | "scala"
  | "security"
  | "solid"
  | "solidity"
  | "storybook"
  | "stylelint"
  | "svelte"
  | "svg"
  | "swift"
  | "tailwind"
  | "taskfile"
  | "terraform"
  | "text"
  | "toml"
  | "typescript"
  | "typescript-config"
  | "typescript-def"
  | "typescript-react"
  | "vercel"
  | "video"
  | "vite"
  | "vitest"
  | "vue"
  | "webpack"
  | "xml"
  | "yaml"
  | "yarn"
  | "zig"
  | "zip";

const FOLDER_ICONS = {
  src: "folder_src",
  app: "folder_app",
  components: "folder_components",
  lib: "folder_lib",
  hooks: "folder_hooks",
  utils: "folder_utils",
  shared: "folder_shared",
  core: "folder_core",
  server: "folder_server",
  client: "folder_client",
  api: "folder_api",
  routes: "folder_routes",
  views: "folder_views",
  controllers: "folder_controllers",
  middleware: "folder_middleware",
  public: "folder_public",
  assets: "folder_assets",
  images: "folder_images",
  fonts: "folder_fonts",
  styles: "folder_styles",
  locales: "folder_locales",
  messages: "folder_messages",
  docs: "folder_docs",
  examples: "folder_examples",
  templates: "folder_templates",
  tests: "folder_tests",
  config: "folder_config",
  scripts: "folder_scripts",
  database: "folder_database",
  prisma: "folder_prisma",
  graphql: "folder_graphql",
  proto: "folder_proto",
  docker: "folder_docker",
  kubernetes: "folder_kubernetes",
  github: "folder_github",
  gitlab: "folder_gitlab",
  vscode: "folder_vscode",
  workflows: "folder_workflows",
  node: "folder_node",
  dist: "folder_dist",
  coverage: "folder_coverage",
  packages: "folder_packages",
  plugins: "folder_plugins",
  themes: "folder_themes",
  cypress: "folder_cypress",
  storybook: "folder_storybook",
  benchmark: "folder_benchmark",
  cargo: "folder_cargo",
  gradle: "folder_gradle",
  nix: "folder_nix",
  firebase: "folder_firebase",
  vercel: "folder_vercel",
  security: "folder_security",
  private: "folder_private",
  temp: "folder_temp",
  upload: "folder_upload",
  download: "folder_download",
  macos: "folder_macos",
  linux: "folder_linux",
  windows: "folder_windows",
  android: "folder_android",
  tauri: "folder_tauri",
  next: "folder_next",
  nuxt: "folder_nuxt",
  git: "folder_git",
  xcode: "folder_xcode",
  // Aliases: the folder name a project uses, not a separate icon.
  test: "folder_tests",
  doc: "folder_docs",
  ".git": "folder_git",
  ".github": "folder_github",
  ".gitlab": "folder_gitlab",
  ".vscode": "folder_vscode",
  ".config": "folder_config",
  ".storybook": "folder_storybook",
  ".cypress": "folder_cypress",
  ".cargo": "folder_cargo",
  ".gradle": "folder_gradle",
  ".nix": "folder_nix",
  node_modules: "folder_node",
  build: "folder_dist",
  out: "folder_dist",
  ".next": "folder_next",
  ".nuxt": "folder_nuxt",
  "src-tauri": "folder_tauri",
} as const;

type FolderAsset = (typeof FOLDER_ICONS)[keyof typeof FOLDER_ICONS];
type FolderIconName = FolderAsset | `${FolderAsset}_open` | "_folder" | "_folder_open";
type CatppuccinIconName = FileIconName | FolderIconName;

const CATPPUCCIN_ICONS_ROOT = "/icons/catppuccin";

function CatppuccinIcon({ name, size = 14 }: IconProps & { name: CatppuccinIconName }) {
  const style = {
    width: size,
    height: size,
    "--catppuccin-icon-light": `url(${CATPPUCCIN_ICONS_ROOT}/latte/${name}.svg)`,
    "--catppuccin-icon-dark": `url(${CATPPUCCIN_ICONS_ROOT}/mocha/${name}.svg)`,
  } as CSSProperties;

  return (
    <span
      aria-hidden="true"
      className="catppuccin-file-icon"
      style={style}
    />
  );
}

function folderIconName(name: string | undefined, open: boolean): CatppuccinIconName {
  const key = name?.toLowerCase();
  const mapped = key ? FOLDER_ICONS[key as keyof typeof FOLDER_ICONS] : undefined;
  if (!mapped) return open ? "_folder_open" : "_folder";
  return `${mapped}${open ? "_open" : ""}` as CatppuccinIconName;
}

export function FolderIcon({ size = 14, open = false, name }: IconProps & { open?: boolean; name?: string }) {
  return <CatppuccinIcon name={folderIconName(name, open)} size={size} />;
}

export function GenericFileIcon({ size = 14 }: IconProps) {
  return <CatppuccinIcon name="_file" size={size} />;
}

const EXTENSION_ICONS: Record<string, CatppuccinIconName> = {
  // Scripting and application languages
  ts: "typescript",
  mts: "typescript",
  cts: "typescript",
  tsx: "typescript-react",
  js: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  jsx: "javascript-react",
  py: "python",
  pyi: "python-config",
  pyc: "python-compiled",
  rb: "ruby",
  rake: "ruby-gem",
  php: "php",
  lua: "lua",
  pl: "perl",
  pm: "perl",
  r: "r",
  go: "go",
  rs: "rust",
  java: "java",
  kt: "kotlin",
  kts: "kotlin",
  swift: "swift",
  scala: "scala",
  groovy: "groovy",
  gvy: "groovy",
  gradle: "gradle",
  dart: "dart",
  zig: "zig",
  ex: "elixir",
  exs: "elixir",
  erl: "erlang",
  hrl: "erlang",
  hs: "haskell",
  lhs: "haskell",
  ml: "ocaml",
  mli: "ocaml",
  clj: "clojure",
  cljs: "clojure",
  cljc: "clojure",
  edn: "clojure",
  jl: "julia",
  m: "matlab",
  nix: "nix",
  vue: "vue",
  svelte: "svelte",
  astro: "astro",
  sol: "solidity",
  asm: "assembly",
  s: "assembly",
  cr: "crystal",
  cob: "cobol",
  cbl: "cobol",
  proto: "proto",
  prisma: "prisma",
  fs: "csharp",
  fsx: "csharp",

  // Native languages and headers
  c: "c",
  h: "c-header",
  cpp: "cpp",
  cxx: "cpp",
  cc: "cpp",
  hpp: "cpp-header",
  hh: "cpp-header",
  hxx: "cpp-header",
  cs: "csharp",
  mm: "cpp",

  // Web, markup, styles
  html: "html",
  htm: "html",
  css: "css",
  scss: "sass",
  sass: "sass",
  less: "less",
  hbs: "javascript",
  handlebars: "javascript",
  mustache: "javascript",
  xml: "xml",
  xsd: "xml",
  xsl: "xml",
  xslt: "xml",
  svg: "svg",
  md: "markdown",
  mdx: "markdown-mdx",
  tex: "text",

  // Data and configuration
  json: "json",
  jsonl: "json",
  jsonc: "json",
  json5: "json",
  jsonld: "json-schema",
  yaml: "yaml",
  yml: "yaml",
  toml: "toml",
  ini: "config",
  cfg: "config",
  conf: "config",
  properties: "config",
  csv: "csv",
  tsv: "csv",
  sql: "database",
  sqlite: "database",
  sqlite3: "database",
  db: "database",
  graphql: "graphql",
  gql: "graphql",
  tf: "terraform",
  tfvars: "terraform",
  hcl: "terraform",
  http: "config",

  // Shells and terminals
  sh: "bash",
  bash: "bash",
  zsh: "bash",
  fish: "bash",
  env: "env",
  ps1: "powershell",
  psm1: "powershell",
  bat: "powershell",
  cmd: "powershell",
  vim: "text",

  // Build and tooling
  mk: "makefile",
  mak: "makefile",
  cmake: "cmake",
  bazel: "bazel",
  bzl: "bazel",
  meson: "meson",
  ninja: "ninja",
  dockerfile: "docker",
  compose: "docker-compose",
  nginx: "nginx",
  htaccess: "apache",
  caddy: "caddy",

  // Documents, media, archives
  pdf: "pdf",
  doc: "ms-word",
  docx: "ms-word",
  odt: "ms-word",
  xls: "ms-excel",
  xlsx: "ms-excel",
  ods: "ms-excel",
  ppt: "ms-powerpoint",
  pptx: "ms-powerpoint",
  odp: "ms-powerpoint",
  txt: "text",
  log: "log",
  png: "image",
  jpg: "image",
  jpeg: "image",
  gif: "image",
  webp: "image",
  bmp: "image",
  ico: "image",
  avif: "image",
  tif: "image",
  tiff: "image",
  mp4: "video",
  m4v: "video",
  webm: "video",
  mov: "video",
  ogv: "video",
  mkv: "video",
  avi: "video",
  mp3: "audio",
  wav: "audio",
  ogg: "audio",
  oga: "audio",
  opus: "audio",
  m4a: "audio",
  aac: "audio",
  flac: "audio",
  weba: "audio",
  ttf: "font",
  otf: "font",
  woff: "font",
  woff2: "font",
  eot: "font",
  zip: "zip",
  tar: "zip",
  gz: "zip",
  tgz: "zip",
  bz2: "zip",
  xz: "zip",
  "7z": "zip",
  rar: "zip",
  exe: "exe",
  msi: "exe",
  dmg: "exe",
  pkg: "exe",
  deb: "exe",
  rpm: "exe",
  apk: "exe",
  pem: "certificate",
  crt: "certificate",
  cer: "certificate",
  key: "key",
  p12: "certificate",
  pfx: "certificate",
  so: "lib",
  dylib: "lib",
  dll: "lib",
  a: "lib",
  lib: "lib",
  nupkg: "nuget",
  lock: "lock",
};

function getSpecialFileIcon(name: string): CatppuccinIconName | undefined {
  if (name === "dockerfile" || name.startsWith("dockerfile.")) return "docker";
  if (name === "containerfile" || name.startsWith("containerfile.")) return "docker";
  if (name === ".dockerignore") return "docker-ignore";
  if (name === "docker-compose.yml" || name === "docker-compose.yaml"
    || name === "compose.yml" || name === "compose.yaml") return "docker-compose";
  if (name === ".env" || name.startsWith(".env.")) return "env";
  if (name === ".envrc") return "envrc";

  if (name === "package.json") return "package-json";
  if (name === "package-lock.json" || name === "npm-shrinkwrap.json") return "npm-lock";
  if (name === "pnpm-lock.yaml") return "pnpm-lock";
  if (name === "yarn.lock") return "yarn";
  if (name === "bun.lock" || name === "bun.lockb") return "bun-lock";
  if (name === "cargo.lock") return "cargo-lock";
  if (name === "cargo.toml") return "cargo";
  if (name === "go.mod" || name === "go.sum") return "go-mod";
  if (name === "gemfile") return "ruby-gem";
  if (name === "gemfile.lock") return "ruby-gem-lock";

  if (name === "cmakelists.txt") return "cmake";
  if (name === "makefile" || name === "gnumakefile") return "makefile";
  if (name === "justfile") return "just";
  if (name === "taskfile" || name === "taskfile.yml" || name === "taskfile.yaml") return "taskfile";
  if (name === "build" || name === "build.bazel" || name === "workspace" || name === "workspace.bazel") return "bazel";
  if (name === "meson.build") return "meson";
  if (name === "build.ninja") return "ninja";
  if (name === "nginx.conf") return "nginx";
  if (name === ".htaccess") return "apache";
  if (name === "caddyfile") return "caddy";

  if (name === "tsconfig.json" || (name.startsWith("tsconfig.") && name.endsWith(".json"))) return "typescript-config";
  if (name === "jsconfig.json") return "javascript-config";
  if (name === "next.config.js" || name === "next.config.mjs" || name === "next.config.cjs" || name === "next.config.ts") return "next";
  if (name.startsWith("vite.config.")) return "vite";
  if (name.startsWith("webpack.config.")) return "webpack";
  if (name.startsWith("rollup.config.")) return "rollup";
  if (name.startsWith("esbuild.config.")) return "esbuild";
  if (name.startsWith("babel.config.") || name === ".babelrc") return "babel";
  if (name.startsWith("tailwind.config.")) return "tailwind";
  if (name.startsWith("postcss.config.")) return "postcss";
  if (name.startsWith("jest.config.")) return "jest";
  if (name.startsWith("vitest.config.")) return "vitest";
  if (name.startsWith("playwright.config.")) return "playwright";
  if (name.startsWith("cypress.config.")) return "cypress";
  if (name.startsWith(".eslintrc") || name.startsWith("eslint.config.")) return "eslint";
  if (name.startsWith(".prettierrc") || name.startsWith("prettier.config.")) return "prettier";
  if (name.startsWith(".stylelintrc") || name.startsWith("stylelint.config.")) return "stylelint";
  if (name.endsWith(".d.ts")) return "typescript-def";
  if (name.endsWith(".config.ts") || name.endsWith(".config.js") || name.endsWith(".config.mjs") || name.endsWith(".config.cjs")) return "config";
  if (name === "vercel.json") return "vercel";
  if (name === "netlify.toml") return "netlify";
  if (name === "firebase.json" || name === ".firebaserc") return "firebase";

  if (name === ".editorconfig") return "editorconfig";
  if (name === ".gitignore" || name === ".gitattributes" || name === ".gitmodules" || name === ".gitconfig") return "git";
  if (name === ".gitlab-ci.yml") return "gitlab";
  if (name === "bitbucket-pipelines.yml") return "bitbucket";

  if (name === "readme" || name.startsWith("readme.")) return "readme";
  if (name === "license" || name === "licence" || name.startsWith("license.") || name.startsWith("licence.") || name === "copying" || name.startsWith("copying.")) return "license";
  if (name === "changelog" || name.startsWith("changelog.")) return "changelog";
  if (name === "contributing" || name.startsWith("contributing.")) return "contributing";
  if (name === "code_of_conduct" || name.startsWith("code_of_conduct.") || name === "code-of-conduct") return "code-of-conduct";
  if (name === "security" || name.startsWith("security.")) return "security";
  if (name === "codeowners") return "codeowners";

  return undefined;
}

export function getFileIcon(name: string, size = 14): React.ReactNode {
  const lower = name.toLowerCase();
  const specialIcon = getSpecialFileIcon(lower);
  if (specialIcon) return <CatppuccinIcon name={specialIcon} size={size} />;

  const ext = lower.split(".").pop() ?? "";
  const icon = EXTENSION_ICONS[ext];
  return icon ? <CatppuccinIcon name={icon} size={size} /> : <GenericFileIcon size={size} />;
}