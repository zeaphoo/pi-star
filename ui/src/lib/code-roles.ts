// The colours code is drawn in, as CSS variables. A theme (the app appearance
// theme, or an explicit code theme in lib/code-theme.ts) defines the variables;
// the highlighter only names the role. That keeps tokenised HTML identical
// across themes: switching one re-resolves the variables instead of
// re-highlighting every open file.
//
// Kept free of Prism and React so both the worker (highlight-core.ts) and the
// chat code block (MermaidBlock.tsx) can share one role table.

export type CodeRole =
  | "foreground"
  | "comment"
  | "meta"
  | "punctuation"
  | "property"
  | "tag"
  | "number"
  | "string"
  | "keyword"
  | "function"
  | "type";

/**
 * Prism's token classes in the order its default stylesheet declares them.
 * A token can carry several (`function class-name`); as in that stylesheet the
 * later declaration wins, so the last entry here wins too.
 */
const TOKEN_ROLES: ReadonlyArray<readonly [string, CodeRole]> = [
  ["comment", "comment"],
  ["prolog", "meta"],
  ["doctype", "meta"],
  ["cdata", "meta"],
  ["punctuation", "punctuation"],
  ["namespace", "type"],
  ["property", "property"],
  ["tag", "tag"],
  ["boolean", "number"],
  ["number", "number"],
  ["constant", "number"],
  ["symbol", "number"],
  ["deleted", "tag"],
  ["selector", "property"],
  ["attr-name", "property"],
  ["string", "string"],
  ["char", "string"],
  ["builtin", "type"],
  ["inserted", "string"],
  ["operator", "foreground"],
  ["entity", "keyword"],
  ["url", "string"],
  ["atrule", "keyword"],
  ["attr-value", "string"],
  ["keyword", "keyword"],
  ["function", "function"],
  ["class-name", "type"],
  ["regex", "string"],
  ["important", "meta"],
  ["variable", "property"],
];

const ROLE_RANK = new Map(TOKEN_ROLES.map(([name], index) => [name, index]));
const ROLE_BY_NAME = new Map(TOKEN_ROLES);

/** A variable per role. `foreground` has no variable of its own: it is the code text colour. */
const ROLE_COLOR: Record<CodeRole, string> = {
  foreground: "var(--code-fg)",
  comment: "var(--code-comment)",
  meta: "var(--code-meta)",
  punctuation: "var(--code-punctuation)",
  property: "var(--code-property)",
  tag: "var(--code-tag)",
  number: "var(--code-number)",
  string: "var(--code-string)",
  keyword: "var(--code-keyword)",
  function: "var(--code-function)",
  type: "var(--code-type)",
};

const BOLD = "bold";
const ITALIC = "italic";

/** The most specific role a token's classes name, or null when none does. */
export function codeRoleFor(classNames: readonly string[]): CodeRole | null {
  let role: CodeRole | null = null;
  let rank = -1;
  for (const name of classNames) {
    const index = ROLE_RANK.get(name);
    if (index !== undefined && index > rank) {
      rank = index;
      role = ROLE_BY_NAME.get(name) ?? null;
    }
  }
  return role;
}

/** The inline declarations for one token: a role's colour, plus bold/italic. */
export function codeStyleFor(classNames: readonly string[]): string {
  const declarations: string[] = [];
  const role = codeRoleFor(classNames);
  if (role) declarations.push(`color:${ROLE_COLOR[role]}`);
  if (classNames.includes(BOLD)) declarations.push("font-weight:600");
  if (classNames.includes(ITALIC)) declarations.push("font-style:italic");
  return declarations.join(";");
}

/**
 * The same roles as a react-syntax-highlighter stylesheet, for the chat code
 * block and the mermaid source view, which render through that library.
 */
export const CODE_THEME_STYLE: Record<string, Record<string, string>> = {
  // Declared (empty) so the library strips it from the rendered class list
  // instead of repeating the prefix it already adds: "token", not "token token".
  token: {},
  'code[class*="language-"]': { color: "var(--code-fg)" },
  // No background here: the code block's own style sets it, and one owner
  // means React can never leave a stale shorthand beside a background-color.
  'pre[class*="language-"]': { color: "var(--code-fg)" },
  ...Object.fromEntries(TOKEN_ROLES.map(([name, role]) => [name, { color: ROLE_COLOR[role] }])),
  [BOLD]: { fontWeight: "600" },
  [ITALIC]: { fontStyle: "italic" },
};