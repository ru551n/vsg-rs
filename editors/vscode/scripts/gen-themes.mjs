// Builds themes/<id>.json for every palette in scripts/palettes.json.
// Run: npm run gen-themes. check-themes.mjs fails when the files on disk differ from this output.
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));

export function loadPalettes() {
  return JSON.parse(readFileSync(join(here, "palettes.json"), "utf8"));
}

const fg = (foreground, fontStyle) => (fontStyle ? { foreground, fontStyle } : { foreground });

export function buildTheme({ name, roles: r }) {
  const rule = (scope, colour, fontStyle) => ({ scope, settings: fg(colour, fontStyle) });
  return {
    name,
    type: "dark",
    semanticHighlighting: true,
    colors: {
      "editor.background": r.bg,
      "editor.foreground": r.fg,
      "editorCursor.foreground": r.fg,
      "editor.selectionBackground": r.bgSel,
      "editor.lineHighlightBackground": r.bgLine,
      "editorLineNumber.foreground": r.gutter,
      "editorLineNumber.activeForeground": r.accent,
      "editorWhitespace.foreground": r.bgLine,
      "editorIndentGuide.background1": r.bgLine,
      "editorIndentGuide.activeBackground1": r.gutter,
      "editorBracketMatch.background": r.bgSel,
      "editorBracketMatch.border": r.accent,
      "editorError.foreground": r.error,
      "editorWarning.foreground": r.warning,
      "editorInfo.foreground": r.info,
      "editorHint.foreground": r.fgDim,
      "editorGutter.addedBackground": r.added,
      "editorGutter.modifiedBackground": r.modified,
      "editorGutter.deletedBackground": r.deleted,
      "editorSuggestWidget.background": r.bgDark,
      "editorSuggestWidget.selectedBackground": r.bgSel,
      "editorHoverWidget.background": r.bgDark,
      "editorWidget.background": r.bgDark,
      "sideBar.background": r.bgDark,
      "sideBarSectionHeader.background": r.bg,
      "activityBar.background": r.bgDark,
      "activityBar.foreground": r.fg,
      "statusBar.background": r.bgDark,
      "statusBar.foreground": r.fg,
      "titleBar.activeBackground": r.bgDark,
      "titleBar.activeForeground": r.fg,
      "tab.activeBackground": r.bg,
      "tab.inactiveBackground": r.bgDark,
      "tab.activeBorderTop": r.accent,
      "panel.background": r.bg,
      "terminal.background": r.bg,
      "terminal.foreground": r.fg,
      "list.activeSelectionBackground": r.bgSel,
      "list.hoverBackground": r.bgLine,
      "input.background": r.bgDark,
      "dropdown.background": r.bgDark,
      focusBorder: r.accent,
    },
    tokenColors: [
      rule(["comment", "comment.line.double-dash.vhdl", "comment.block.vhdl"], r.comment, "italic"),
      rule("string.quoted.double.vhdl", r.string),
      rule("constant.character.vhdl", r.string),
      rule("constant.character.escape.vhdl", r.escape),
      rule(
        [
          "constant.numeric.decimal.vhdl",
          "constant.numeric.based.vhdl",
          "constant.numeric.bit-string.vhdl",
        ],
        r.number,
      ),
      rule("keyword.control.vhdl", r.keyword),
      rule("keyword.other.vhdl", r.keyword),
      rule("storage.type.vhdl", r.keyword),
      rule("keyword.operator.word.vhdl", r.keyword),
      rule(["keyword.operator.comparison.vhdl", "keyword.operator.arithmetic.vhdl"], r.operator),
      rule("keyword.operator.assignment.vhdl", r.operator),
      rule("support.other.attribute.vhdl", r.attribute, "italic"),
    ],
    semanticTokenColors: {
      variable: r.fg,
      "variable.readonly": r.constant,
      parameter: fg(r.parameter, "italic"),
      property: r.property,
      enumMember: r.constant,
      function: r.function,
      type: fg(r.type, "italic"),
      class: r.type,
      namespace: r.namespace,
      struct: fg(r.type, "italic"),
      enum: fg(r.type, "italic"),
      operator: r.operator,
    },
  };
}

export const render = (palette) => `${JSON.stringify(buildTheme(palette), null, 2)}\n`;

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  for (const palette of loadPalettes()) {
    writeFileSync(join(here, "..", "themes", `${palette.id}.json`), render(palette));
  }
}
