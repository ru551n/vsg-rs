//! Width-aware document model and printer.
//!
//! The formatter lowers the syntax tree into a [`Doc`], a tree of atoms (tokens and comments),
//! line breaks, indentation and groups. The printer decides, group by group, whether a group is
//! printed flat or broken, using the classic "does the rest of this line fit" test
//! (Wadler/Oppen/Prettier family). The decision for a group depends only on the document and the
//! configured width, so output is deterministic.
//!
//! Spacing between atoms is carried by the atoms themselves (`space`), so a [`Doc::Line`] prints
//! nothing when flat and a newline when broken.
//!
//! Comments that trail a token are printed as line suffixes: they never count towards the width
//! when deciding a layout, and a pending suffix always ends the line before any further atom is
//! printed, so a comment can never swallow code.

use std::rc::Rc;

pub type GroupId = usize;

#[derive(Debug, Clone)]
pub enum Doc {
    Nil,
    /// Text that is never split. `space`: separate from a preceding atom on the same line.
    Atom {
        text: Rc<[u8]>,
        width: usize,
        space: bool,
    },
    /// Nothing when flat, a newline when the enclosing group is broken.
    Line,
    /// `n` spaces between two atoms on the same line (nothing at the start of a line).
    Space(usize),
    /// Always a newline; forces all enclosing groups to break.
    Hard,
    /// The given number of blank lines; forces all enclosing groups to break.
    Blank(u8),
    /// Zero-width marker that forces all enclosing groups to break.
    BreakParent,
    /// End-of-line text (a trailing comment). Does not count towards the width.
    Suffix(Rc<[u8]>),
    Concat(Vec<Doc>),
    /// Increase indentation by one level for lines started inside.
    Indent(Box<Doc>),
    /// Indent lines started inside to the column where the first atom inside is printed.
    Align(Box<Doc>),
    Group {
        id: GroupId,
        doc: Box<Doc>,
        broken: bool,
    },
    /// Chooses a document depending on how group `id` was printed.
    IfBreak {
        id: GroupId,
        broken: Box<Doc>,
        flat: Box<Doc>,
    },
    /// Two layouts of the same tokens: `flat` when its first line fits (and always when the
    /// enclosing group is flat), `broken` otherwise.
    Choice {
        flat: Box<Doc>,
        broken: Box<Doc>,
    },
    /// A value after `:=` or `=>`, in order of preference: flat on the current line; flat on an
    /// indented continuation line; folded inside itself with its first line on the current
    /// line; folded on a continuation line.
    /// `forced`: the value contains a forced line break (computed when printing).
    Hug {
        value: Box<Doc>,
        forced: bool,
    },
    /// Items separated by line breaks (odd positions); a separator breaks only when the item
    /// after it does not fit on the current line.
    Fill(Vec<Doc>),
}

impl Doc {
    pub fn indent(d: Doc) -> Doc {
        Doc::Indent(Box::new(d))
    }
    /// `levels` nested [`Doc::indent`]s.
    pub fn indent_n(levels: usize, d: Doc) -> Doc {
        (0..levels).fold(d, |d, _| Doc::indent(d))
    }
    pub fn align(d: Doc) -> Doc {
        Doc::Align(Box::new(d))
    }

    /// Compute `broken` for every group: a group containing a forced break is always broken.
    fn propagate(&mut self) -> bool {
        match self {
            Doc::Hard | Doc::Blank(_) | Doc::BreakParent => true,
            Doc::Nil | Doc::Line | Doc::Suffix(_) | Doc::Space(_) => false,
            Doc::Atom { text, .. } => text.contains(&b'\n'),
            Doc::Concat(v) | Doc::Fill(v) => v.iter_mut().fold(false, |acc, d| d.propagate() | acc),
            Doc::Indent(d) | Doc::Align(d) => d.propagate(),
            // A forced break inside does not force the line break in front of the value.
            Doc::Hug { value, forced } => {
                *forced = value.propagate();
                false
            }
            Doc::Group { doc, broken, .. } => {
                *broken |= doc.propagate();
                *broken
            }
            // Only the selected branch is printed, and selection depends on the group itself,
            // so forced breaks inside a branch do not propagate.
            Doc::IfBreak { broken, flat, .. } => {
                broken.propagate();
                flat.propagate();
                false
            }
            Doc::Choice { flat, broken } => flat.propagate() & broken.propagate(),
        }
    }

    /// Whether printing starts with a forced line break (an own-line comment).
    fn starts_with_break(&self) -> bool {
        match self {
            Doc::Hard | Doc::Blank(_) => true,
            Doc::Concat(v) | Doc::Fill(v) => v
                .iter()
                .find(|d| !matches!(d, Doc::Nil) && !matches!(d, Doc::Concat(c) if c.is_empty()))
                .is_some_and(Doc::starts_with_break),
            Doc::Indent(d) | Doc::Align(d) | Doc::Group { doc: d, .. } => d.starts_with_break(),
            _ => false,
        }
    }

    fn first_atom_space(&self) -> Option<bool> {
        match self {
            Doc::Atom { space, .. } => Some(*space),
            Doc::Concat(v) | Doc::Fill(v) => v.iter().find_map(Doc::first_atom_space),
            Doc::Indent(d)
            | Doc::Align(d)
            | Doc::Hug { value: d, .. }
            | Doc::Group { doc: d, .. } => d.first_atom_space(),
            Doc::Choice { flat, .. } => flat.first_atom_space(),
            Doc::Line | Doc::Hard | Doc::Blank(_) => Some(false),
            _ => None,
        }
    }
}

pub struct PrintOptions {
    pub width: usize,
    pub indent: usize,
    /// Indent with tabs (one per level) and align with spaces (VSG `smart_tabs`). A tab is
    /// assumed to be `indent` columns wide when measuring.
    pub tabs: bool,
    /// Continuation lines are indented one level instead of aligned (VSG `align_left: yes`,
    /// `align_paren: no`).
    pub indent_continuations: bool,
}

impl From<&crate::FormatConfig> for PrintOptions {
    fn from(cfg: &crate::FormatConfig) -> PrintOptions {
        PrintOptions {
            width: cfg.width,
            indent: cfg.indent,
            tabs: cfg.tabs,
            indent_continuations: cfg.indent_continuations,
        }
    }
}

#[derive(Clone, Copy)]
struct Cmd<'a> {
    indent: usize,
    /// The part of `indent` that consists of whole indentation levels (printed as tabs).
    levels: usize,
    flat: bool,
    doc: &'a Doc,
    /// Position inside a [`Doc::Fill`] still to be printed.
    fill: usize,
}

struct Printer<'a> {
    opts: &'a PrintOptions,
    out: Vec<u8>,
    col: usize,
    /// Newlines requested but not yet written (0, 1 or 2) and the indentation to use.
    pending: u8,
    pending_indent: usize,
    pending_levels: usize,
    at_start: bool,
    /// Indentation of the current line, and its part made of whole levels.
    line_indent: usize,
    line_levels: usize,
    suffix: Vec<u8>,
    modes: Vec<bool>,
}

/// Print `doc`. The result uses `\n` line endings and ends with exactly one newline
/// (unless nothing was printed at all).
pub fn print(mut doc: Doc, groups: usize, opts: &PrintOptions) -> Vec<u8> {
    doc.propagate();
    let mut p = Printer {
        opts,
        out: Vec::new(),
        col: 0,
        pending: 0,
        pending_indent: 0,
        pending_levels: 0,
        at_start: true,
        line_indent: 0,
        line_levels: 0,
        suffix: Vec::new(),
        modes: vec![false; groups],
    };
    let mut stack = vec![Cmd {
        indent: 0,
        levels: 0,
        flat: false,
        doc: &doc,
        fill: 0,
    }];
    while let Some(cmd) = stack.pop() {
        p.step(cmd, &mut stack);
    }
    p.out.append(&mut p.suffix);
    if !p.out.is_empty() {
        p.out.push(b'\n');
    }
    p.out
}

impl<'a> Printer<'a> {
    fn step(&mut self, cmd: Cmd<'a>, stack: &mut Vec<Cmd<'a>>) {
        match cmd.doc {
            Doc::Nil | Doc::BreakParent => {}
            Doc::Space(n) => {
                if self.pending == 0 && !self.at_start && self.suffix.is_empty() {
                    self.out.extend(std::iter::repeat_n(b' ', *n));
                    self.col += n;
                }
            }
            Doc::Atom { text, width, space } => self.atom(text, *width, *space, &cmd),
            Doc::Line if cmd.flat => {}
            Doc::Line | Doc::Hard => self.newline(1, cmd.indent, cmd.levels),
            Doc::Blank(n) => self.newline(n.saturating_add(1), cmd.indent, cmd.levels),
            Doc::Suffix(s) => self.suffix.extend_from_slice(s),
            Doc::Concat(v) => stack.extend(v.iter().rev().map(|doc| Cmd {
                doc,
                fill: 0,
                ..cmd
            })),
            Doc::Fill(v) => {
                let Some(item) = v.get(cmd.fill) else { return };
                stack.push(Cmd {
                    fill: cmd.fill + 1,
                    ..cmd
                });
                // The last item must also leave room for what follows the fill.
                let rest_for = |i: usize, stack: &[Cmd<'a>]| -> usize {
                    if i + 1 == v.len() { stack.len() - 1 } else { 0 }
                };
                let flat = if cmd.flat {
                    true
                } else if cmd.fill % 2 == 1 {
                    // A separator stays flat when the next item fits after it.
                    v.get(cmd.fill + 1).is_none_or(|next| {
                        let rest = &stack[..rest_for(cmd.fill + 1, stack)];
                        self.fits(
                            Cmd {
                                flat: true,
                                doc: next,
                                fill: 0,
                                ..cmd
                            },
                            rest,
                        )
                    })
                } else {
                    let rest = &stack[..rest_for(cmd.fill, stack)];
                    self.fits(
                        Cmd {
                            flat: true,
                            doc: item,
                            fill: 0,
                            ..cmd
                        },
                        rest,
                    )
                };
                stack.push(Cmd {
                    flat,
                    doc: item,
                    fill: 0,
                    ..cmd
                });
            }
            Doc::Indent(d) => stack.push(Cmd {
                indent: cmd.indent + self.opts.indent,
                levels: deeper(cmd.indent, cmd.levels, self.opts.indent),
                doc: d,
                ..cmd
            }),
            Doc::Align(d) if self.opts.indent_continuations => stack.push(Cmd {
                indent: cmd.indent + self.opts.indent,
                levels: deeper(cmd.indent, cmd.levels, self.opts.indent),
                doc: d,
                ..cmd
            }),
            Doc::Align(d) => {
                let (target, line_indent, line_levels) =
                    if self.pending > 0 || self.at_start || !self.suffix.is_empty() {
                        (cmd.indent, cmd.indent, cmd.levels)
                    } else {
                        let space = usize::from(d.first_atom_space().unwrap_or(false));
                        (self.col + space, self.line_indent, self.line_levels)
                    };
                // Bounded alignment: past 40% of the width, continuation lines use a fixed
                // double indent instead, so deep alignment never leaves no room on the right.
                let (indent, levels) = if target * 5 > self.opts.width * 2 {
                    let once = deeper(line_indent, line_levels, self.opts.indent);
                    (
                        line_indent + 2 * self.opts.indent,
                        deeper(line_indent + self.opts.indent, once, self.opts.indent),
                    )
                } else {
                    (target, line_levels)
                };
                stack.push(Cmd {
                    indent,
                    levels,
                    doc: d,
                    ..cmd
                });
            }
            Doc::Group { id, doc, broken } => {
                let flat = cmd.flat
                    || (!broken
                        && self.fits(
                            Cmd {
                                flat: true,
                                doc,
                                ..cmd
                            },
                            stack,
                        ));
                self.modes[*id] = !flat;
                stack.push(Cmd { flat, doc, ..cmd });
            }
            Doc::IfBreak { id, broken, flat } => {
                let doc = if self.modes[*id] { broken } else { flat };
                stack.push(Cmd { doc, ..cmd });
            }
            Doc::Hug { value, forced } => {
                let continuation = cmd.indent + self.opts.indent;
                let levels = deeper(cmd.indent, cmd.levels, self.opts.indent);
                let flat = Cmd {
                    flat: true,
                    doc: value,
                    ..cmd
                };
                let broken = Cmd {
                    flat: false,
                    ..flat
                };
                if cmd.flat || (!forced && self.fits(flat, stack)) {
                    stack.push(flat);
                } else if !forced && self.fits_from(continuation, true, flat, stack) {
                    self.newline(1, continuation, levels);
                    stack.push(Cmd {
                        indent: continuation,
                        levels,
                        ..flat
                    });
                } else if !value.starts_with_break() && self.fits(broken, stack) {
                    stack.push(broken);
                } else {
                    self.newline(1, continuation, levels);
                    stack.push(Cmd {
                        indent: continuation,
                        levels,
                        ..broken
                    });
                }
            }
            Doc::Choice { flat, broken } => {
                let fits = cmd.flat || self.fits(Cmd { doc: flat, ..cmd }, stack);
                let doc = if fits { flat } else { broken };
                stack.push(Cmd { doc, ..cmd });
            }
        }
    }

    fn newline(&mut self, n: u8, indent: usize, levels: usize) {
        if self.out.is_empty() {
            return;
        }
        self.pending = self.pending.max(n);
        self.pending_indent = indent;
        self.pending_levels = levels;
    }

    fn indent_to(&mut self, indent: usize, levels: usize) {
        if self.opts.tabs && self.opts.indent > 0 {
            self.out
                .extend(std::iter::repeat_n(b'\t', levels / self.opts.indent));
            self.out.extend(std::iter::repeat_n(b' ', indent - levels));
        } else {
            self.out.extend(std::iter::repeat_n(b' ', indent));
        }
        self.col = indent;
        self.line_indent = indent;
        self.line_levels = levels;
    }

    fn atom(&mut self, text: &[u8], width: usize, space: bool, cmd: &Cmd<'_>) {
        if !self.suffix.is_empty() {
            self.out.append(&mut self.suffix);
            if self.pending == 0 {
                self.pending = 1;
                self.pending_indent = cmd.indent;
                self.pending_levels = cmd.levels;
            }
        }
        if self.pending > 0 {
            for _ in 0..self.pending {
                self.out.push(b'\n');
            }
            self.pending = 0;
            self.indent_to(self.pending_indent, self.pending_levels);
        } else if self.at_start {
            self.indent_to(cmd.indent, cmd.levels);
        } else if space {
            self.out.push(b' ');
            self.col += 1;
        }
        self.at_start = false;
        self.out.extend_from_slice(text);
        // Multi-line atoms (block comments) restart the column count; `width` is then the
        // width of their last line.
        self.col = if text.contains(&b'\n') {
            width
        } else {
            self.col + width
        };
    }

    /// Would `next` fit flat on the current line, followed by the rest of the line?
    fn fits(&self, next: Cmd<'a>, rest: &[Cmd<'a>]) -> bool {
        if self.pending > 0 {
            self.fits_from(self.pending_indent, true, next, rest)
        } else {
            self.fits_from(self.col, self.at_start, next, rest)
        }
    }

    /// [`Printer::fits`], starting at a given column (at the start of a line if `at_start`).
    fn fits_from(
        &self,
        mut col: usize,
        mut at_start: bool,
        next: Cmd<'a>,
        rest: &[Cmd<'a>],
    ) -> bool {
        let mut rest_idx = rest.len();
        let mut work = vec![next];
        loop {
            if col > self.opts.width {
                return false;
            }
            let cmd = match work.pop() {
                Some(c) => c,
                None if rest_idx == 0 => return true,
                None => {
                    rest_idx -= 1;
                    rest[rest_idx]
                }
            };
            match cmd.doc {
                Doc::Nil | Doc::Suffix(_) | Doc::BreakParent => {}
                Doc::Space(n) => {
                    if !at_start {
                        col += n;
                    }
                }
                Doc::Atom { width, space, text } => {
                    if text.contains(&b'\n') {
                        return false;
                    }
                    if at_start {
                        col = col.max(cmd.indent);
                    } else if *space {
                        col += 1;
                    }
                    at_start = false;
                    col += width;
                }
                Doc::Line if cmd.flat => {}
                Doc::Line | Doc::Hard | Doc::Blank(_) => return true,
                Doc::Concat(v) | Doc::Fill(v) => {
                    work.extend(v.iter().rev().map(|doc| Cmd {
                        doc,
                        fill: 0,
                        ..cmd
                    }));
                }
                Doc::Indent(d) | Doc::Align(d) => work.push(Cmd { doc: d, ..cmd }),
                // Groups after `next` are measured in the mode of their enclosing command, so
                // an undecided group later on the line ends the measurement at its first line
                // break.
                Doc::Group { doc, broken, .. } => {
                    if cmd.flat && *broken {
                        return false;
                    }
                    work.push(Cmd {
                        doc,
                        flat: cmd.flat && !broken,
                        ..cmd
                    });
                }
                // Groups not yet printed (including the one being measured) read as flat.
                Doc::IfBreak { id, broken, flat } => {
                    let doc = if self.modes[*id] { broken } else { flat };
                    work.push(Cmd { doc, ..cmd });
                }
                // Later on the line, a hugged value may still move to the next line.
                Doc::Hug { value, .. } if cmd.flat => work.push(Cmd { doc: value, ..cmd }),
                Doc::Hug { .. } => return true,
                // Later on the line, a choice may still take its most broken alternative.
                Doc::Choice { flat, broken } => {
                    let doc = if cmd.flat { flat } else { broken };
                    work.push(Cmd { doc, ..cmd });
                }
            }
        }
    }
}

/// The levels part of an indentation one level deeper: alignment spaces stay spaces.
fn deeper(indent: usize, levels: usize, step: usize) -> usize {
    if levels == indent {
        levels + step
    } else {
        levels
    }
}

/// Tab stop used when measuring text containing tabs (only comments can contain tabs).
pub const TAB_WIDTH: usize = 4;

/// Display width of `text` printed at column `start`: one column per character (per UTF-8
/// scalar when the file is valid UTF-8, otherwise per Latin-1 byte); tabs advance to the next
/// multiple of [`TAB_WIDTH`]. For multi-line text, the width of the last line.
pub fn display_width(text: &[u8], utf8: bool, start: usize) -> usize {
    let mut col = start;
    let mut base = start;
    for &b in text {
        match b {
            b'\t' => col = (col / TAB_WIDTH + 1) * TAB_WIDTH,
            b'\n' => {
                col = 0;
                base = 0;
            }
            _ if utf8 && (b & 0xC0) == 0x80 => {}
            _ => col += 1,
        }
    }
    col - base
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atom(s: &str, space: bool) -> Doc {
        Doc::Atom {
            text: s.as_bytes().into(),
            width: s.len(),
            space,
        }
    }

    fn call(n: usize) -> Doc {
        let mut inner = vec![Doc::Line];
        for i in 0..n {
            if i > 0 {
                inner.push(atom(",", false));
                inner.push(Doc::Line);
            }
            inner.push(atom(&format!("arg{i}"), i > 0));
        }
        Doc::Concat(vec![
            atom("f", false),
            Doc::Group {
                id: 0,
                broken: false,
                doc: Box::new(Doc::Concat(vec![
                    atom("(", false),
                    Doc::indent(Doc::Concat(inner)),
                    Doc::Line,
                    atom(")", false),
                ])),
            },
        ])
    }

    fn opts(width: usize) -> PrintOptions {
        PrintOptions {
            width,
            indent: 2,
            tabs: false,
            indent_continuations: false,
        }
    }

    #[test]
    fn group_flat_or_broken_by_width() {
        assert_eq!(print(call(3), 1, &opts(80)), b"f(arg0, arg1, arg2)\n");
        let out = print(call(3), 1, &opts(10));
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "f(\n  arg0,\n  arg1,\n  arg2\n)\n"
        );
    }

    #[test]
    fn exact_width_boundary() {
        // Flat width is 19.
        assert_eq!(print(call(3), 1, &opts(19)), b"f(arg0, arg1, arg2)\n");
        assert_ne!(print(call(3), 1, &opts(18)), b"f(arg0, arg1, arg2)\n");
    }

    #[test]
    fn suffix_forces_newline_before_next_atom() {
        let d = Doc::Concat(vec![
            atom("a", false),
            Doc::Suffix(b" -- c".as_slice().into()),
            atom("b", true),
        ]);
        assert_eq!(print(d, 0, &opts(80)), b"a -- c\nb\n");
    }

    #[test]
    fn suffix_does_not_count_towards_width() {
        // The group fits in 20 columns; the trailing comment must not make it break.
        let d = Doc::Concat(vec![
            call(3),
            Doc::Suffix(b" -- a very long comment".as_slice().into()),
        ]);
        let out = String::from_utf8(print(d, 1, &opts(20))).unwrap();
        assert_eq!(out, "f(arg0, arg1, arg2) -- a very long comment\n");
    }

    #[test]
    fn widths() {
        assert_eq!(display_width("héllo".as_bytes(), true, 0), 5);
        assert_eq!(display_width(&[b'h', 0xE9], false, 0), 2);
        assert_eq!(display_width(b"\tx", true, 1), 4);
        assert_eq!(display_width(b"ab\ncde", true, 7), 3);
    }
}
