# File icon themes

The tree uses one of three sets. Each set keeps its own license beside its SVGs.

| Theme | Source | License | Layout |
| --- | --- | --- | --- |
| Catppuccin | [catppuccin/vscode-icons](https://github.com/catppuccin/vscode-icons) | MIT | `catppuccin/latte`, `catppuccin/mocha` |
| Material Icon Theme | [material-extensions/vscode-material-icon-theme](https://github.com/material-extensions/vscode-material-icon-theme) 5.40.0 | MIT | `material/` |
| Seti UI | [jesseweed/seti-ui](https://github.com/jesseweed/seti-ui) | MIT | `seti/light`, `seti/dark` |
| VS Code Icons | [vscode-icons/vscode-icons](https://github.com/vscode-icons/vscode-icons) | MIT | `vscode/` |

Catppuccin chooses `latte` or `mocha` in CSS (`html.dark`), so the same markup
works for both flavours. Material and VS Code Icons publish a manifest with
their own file-name, extension and folder associations; `src/lib/icon-themes.ts`
resolves those manifests and only the referenced SVGs are vendored here.

The manifests live in `src/lib/icon-manifests/`. VS Code Icons' manifest is
generated from its `supportedExtensions.ts` and `supportedFolders.ts` (the npm
package's generated manifest omits the language-derived extensions), with
missing icon paths pointed at the default file/folder icons.

Seti UI ships LESS suffix/substring rules rather than a manifest. Its rules are
vendored in `seti-icons.json` (last match wins, like the LESS), and each SVG is
copied twice: `light` uses `#4d5a5e` ink for unfilled shapes and `dark` uses
`#d4d7d6`, so the monochrome parts stay visible on either background. Shapes
with their own fill keep it.