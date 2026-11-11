import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import React from "react";
import ts from "typescript";

const source = await readFile(new URL("./FileViewer.tsx", import.meta.url), "utf8");

test("lightweight source rows are skipped for highlighted, diff, and preview views", () => {
  // Execute the source-view calculations without mounting the file-fetching component.
  const file = ts.createSourceFile("FileViewer.tsx", source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const viewer = file.statements.find((node) => ts.isFunctionDeclaration(node) && node.name?.text === "TextFileViewer");
  const calculations = viewer.body.statements.filter((node) =>
    ts.isVariableStatement(node) && node.declarationList.declarations.some((declaration) =>
      ["isBinary", "isDelimitedTable", "viewerContent", "sourceLines", "language", "canHighlightSource", "isHtml", "isMarkdown", "hasPreview", "effectiveDisplayMode", "useLightweightSource"].includes(declaration.name.getText(file)),
    ),
  ).map((node) => node.getText(file)).join("\n");
  const sourceRows = file.statements.find((node) => ts.isFunctionDeclaration(node) && node.name?.text === "sourceRows");
  assert.ok(sourceRows, "sourceRows is declared");
  const { outputText } = ts.transpileModule(`
    return (data, displayMode, hasGitDiff = false, isDeletedDiff = false, wrapLines = false, filePath = "/tmp/file.txt") => {
      const isDelimitedTablePath = (path) => /\\.(csv|tsv)$/i.test(path);
      const SOURCE_HIGHLIGHT_MAX_LINES = 10_000;
      const SOURCE_PINNED_MAX_LINES = 1_000;
      const HTML_PREVIEW_MAX_BYTES = 10 * 1024 * 1024;
      const FILE_CODE_STYLE = {};
      const FILE_SOURCE_LINE_CONTENT_STYLE = {};
      const FILE_LINE_NUMBER_STYLE = {};
      const FILE_PINNED_LINE_NUMBER_STYLE = {};
      ${sourceRows.getText(file)}
      ${calculations}
      return {
        useLightweightSource,
        rows: useLightweightSource ? sourceRows(sourceLines, null, FILE_LINE_NUMBER_STYLE, wrapLines) : null,
      };
    };
  `, { compilerOptions: { jsx: ts.JsxEmit.React } });
  const render = new Function("React", "useMemo", outputText)(React, (calculate) => calculate());

  // Never hand a rendered row array to assert: a failing assertion inspects the
  // actual value, and util.inspect on thousands of React elements grows at
  // hundreds of MB/s until the process is killed. Compare counts instead.
  const rowsOf = (...args) => {
    const rows = render(...args).rows;
    return rows === null ? null : rows.length;
  };
  const rowsFor = (lineCount, language = "rust") => ({ content: "line\n".repeat(lineCount - 1), language });
  const large = rowsFor(10_001);
  assert.equal(large.content.split("\n").length, 10_001);

  // A file with a grammar highlights up to SOURCE_HIGHLIGHT_MAX_LINES lines;
  // past the cap it falls back to plain rows.
  assert.equal(rowsOf(rowsFor(10_000), "source"), null);
  assert.equal(rowsOf(rowsFor(10_001), "source"), 10_001);
  // Plain text has no grammar to run, so it never takes the highlight path.
  assert.equal(rowsOf(rowsFor(10, "text"), "source"), 10);
  assert.equal(rowsOf(large, "diff", true), null);
  assert.equal(rowsOf(large, "source", true, true), null);
  for (const language of ["html", "markdown"]) {
    assert.equal(rowsOf({ ...large, language }, "preview"), null);
  }
  // A CSV/TSV table previews a loaded prefix too; markdown waits for the whole file.
  assert.equal(rowsOf({ ...large, truncated: true }, "preview", false, false, false, "/tmp/data.csv"), null);
  // A binary file has no Preview, whatever its extension (#1401).
  assert.equal(rowsOf({ ...large, binary: true }, "preview", false, false, false, "/tmp/data.csv"), 10_001);
  assert.equal(rowsOf({ ...large, language: "html", binary: true }, "preview"), 10_001);
  assert.equal(rowsOf({ ...large, language: "markdown", truncated: true }, "preview"), 10_001);
  // A partly loaded HTML file previews too: the preview fetches it whole.
  assert.equal(rowsOf({ ...large, language: "html", truncated: true, size: 900 * 1024 }, "preview"), null);

  // The plain fallback keeps one row per line, its number and its text.
  const first = render(large, "preview").rows[0];
  assert.equal(first.props["data-line-number"], 1);
  assert.equal(first.props.children[1].props.children, "line");
  assert.equal(
    render(large, "source", false, false, true).rows[0].props.children[1].props.style.whiteSpace,
    "pre-wrap",
  );
});

test("the diff view wraps when the viewer wraps, and the key reaches the diff", () => {
  const diffView = source.slice(source.indexOf("function DiffView"), source.indexOf("function ImageViewer"));
  assert.notEqual(source.indexOf("function DiffView"), -1);

  // Wrapped: the box stops being the widest line's width and the row's text
  // column gives way instead of pushing a horizontal scrollbar.
  assert.match(diffView, /width: wrapLines \? "100%" : "max-content"/);
  assert.match(diffView, /whiteSpace: wrapLines \? "pre-wrap" : "pre"/);
  assert.match(diffView, /overflowWrap: wrapLines \? "anywhere" : "normal"/);
  assert.match(diffView, /flexShrink: wrapLines \? 1 : 0/);
  // Unwrapped stays exactly as before: one line per row, sideways scroll. The
  // +/- marker column never shrinks either way.
  assert.match(diffView, /padding: "0 6px",[\s\S]{0,160}flexShrink: 0/);
  assert.match(source, /<DiffView patch=\{gitDiff\.patch!\} wrapLines=\{wrapLines\} language=\{diffLanguage\} \/>/);

  // The wrap control is offered for the diff as well as the source, never for a
  // rendered preview (markdown, HTML, table), where there are no lines to wrap.
  assert.match(source, /\(effectiveDisplayMode === "source" \|\| effectiveDisplayMode === "diff"\) && !isBinary/);
});

test("partly loaded HTML previews up to the HTML cap; partly loaded markdown does not", () => {
  const start = source.indexOf("  const hasPreview = ");
  const statement = source.slice(start, source.indexOf(";\n", start) + 1);
  const hasPreview = new Function("data", "isHtml", "isMarkdown", "isDelimitedTable", "HTML_PREVIEW_MAX_BYTES", `${statement}\nreturn hasPreview;`);
  const cap = 10 * 1024 * 1024;
  const partial = { truncated: true, size: 900 * 1024 };

  assert.equal(hasPreview(partial, true, false, false, cap), true);
  assert.equal(hasPreview({ ...partial, size: cap + 1 }, true, false, false, cap), false);
  assert.equal(hasPreview({ truncated: false, size: cap + 1 }, true, false, false, cap), true);
  assert.equal(hasPreview(partial, false, true, false, cap), false);
  assert.equal(hasPreview({ ...partial, size: cap + 1 }, false, false, true, cap), true);
});

test("the line-number gutter is pinned only in views of at most SOURCE_PINNED_MAX_LINES rows", () => {
  // Evaluate the module's own style constants and the source-view calculation
  // instead of matching the component's text.
  const file = ts.createSourceFile("FileViewer.tsx", source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const constants = (...names) => {
    const statements = names.map((name) => {
      const statement = file.statements.find((node) =>
        ts.isVariableStatement(node) && node.declarationList.declarations.some((declaration) =>
          ts.isIdentifier(declaration.name) && declaration.name.text === name,
        ),
      );
      assert.ok(statement, `${name} is declared in the module`);
      return statement.getText(file);
    });
    const { outputText } = ts.transpileModule(`${statements.join("\n")}\nreturn [${names.join(", ")}];`, { compilerOptions: { target: ts.ScriptTarget.ES2022 } });
    return new Function(outputText)();
  };
  const [codeStyle, gutterStyle, pinnedStyle, pinnedMax] = constants(
    "FILE_CODE_STYLE",
    "FILE_LINE_NUMBER_STYLE",
    "FILE_PINNED_LINE_NUMBER_STYLE",
    "SOURCE_PINNED_MAX_LINES",
  );

  const viewer = file.statements.find((node) => ts.isFunctionDeclaration(node) && node.name?.text === "TextFileViewer");
  const statement = viewer.body.statements.find((node) =>
    ts.isVariableStatement(node) && node.declarationList.declarations.some((declaration) =>
      declaration.name.getText(file) === "sourceLineNumberStyle",
    ),
  );
  assert.ok(statement, "sourceLineNumberStyle is computed in TextFileViewer");
  const { outputText } = ts.transpileModule(`
    return (lineCount) => {
      const sourceLines = new Array(lineCount);
      const SOURCE_PINNED_MAX_LINES = pinnedMax;
      const FILE_LINE_NUMBER_STYLE = gutterStyle;
      const FILE_PINNED_LINE_NUMBER_STYLE = pinnedStyle;
      ${statement.getText(file)}
      return sourceLineNumberStyle;
    };
  `, { compilerOptions: { target: ts.ScriptTarget.ES2022 } });
  const numberStyleFor = new Function("gutterStyle", "pinnedStyle", "pinnedMax", outputText)(gutterStyle, pinnedStyle, pinnedMax);

  // The pinned gutter is the same column, pinned to the left edge of the
  // viewer's `overflow: auto` content box above the tokens that slide behind it.
  for (const [key, value] of Object.entries(gutterStyle)) assert.equal(pinnedStyle[key], value, key);
  assert.equal(pinnedStyle.position, "sticky");
  assert.equal(pinnedStyle.left, 0);
  assert.ok(pinnedStyle.zIndex >= 1, "the pinned gutter must stack above the scrolling text");

  // A 10,000-row file highlights, but only short views pin every gutter cell:
  // the browser repositions sticky cells on each scroll.
  assert.equal(pinnedMax, 1_000);
  assert.equal(numberStyleFor(pinnedMax), pinnedStyle);
  assert.equal(numberStyleFor(pinnedMax + 1), gutterStyle);
  assert.match(source, /visible\.size <= SOURCE_PINNED_MAX_LINES/);
  assert.match(source, /sourceRows\(sourceLines, highlightedSourceLines, sourceLineNumberStyle, wrapLines\)/);
  assert.match(source, /sourceRows\(sourceLines, null, FILE_LINE_NUMBER_STYLE, wrapLines\)/);

  // The gutter repeats the code line's height in px, so a code-font change
  // without it puts the numbers off the line boxes they belong to.
  assert.equal(parseFloat(gutterStyle.lineHeight), Number((codeStyle.fontSize * codeStyle.lineHeight).toFixed(1)));
});
