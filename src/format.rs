//! Lowering of the concrete syntax tree into a layout [`Doc`].
//!
//! Every token of the tree is emitted exactly once and in source order; the formatter only
//! decides the whitespace between tokens. Structure decides where lines start and how they are
//! indented; groups decide where long constructs are folded. See `docs/formatting.md` and
//! `docs/line-folding.md` for the policy.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use vhdl_syntax::standard::VHDLStandard;
use vhdl_syntax::syntax::{NodeKind as N, SyntaxElement, SyntaxNode, SyntaxToken};
use vhdl_syntax::tokens::token::requires_separator;
use vhdl_syntax::tokens::{Keyword as Kw, TokenKind as T, TriviaPiece};

use crate::Parsed;
use crate::config::{FormatConfig, KeywordCase};
use crate::doc::{Doc, GroupId, display_width};
use crate::indent::Part;

pub(crate) struct Builder<'a> {
    cfg: &'a FormatConfig,
    parsed: &'a Parsed,
    utf8: bool,
    standard: VHDLStandard,
    groups: usize,
    /// Tokens (by text offset) whose leading comments were already emitted.
    comments_done: HashSet<usize>,
    /// Tokens (by text offset) that start an item; blank lines before them are kept.
    item_starts: HashSet<usize>,
    /// Alignment padding requested for tokens (by text offset).
    pads: HashMap<usize, Pad>,
    /// Blank-line policy for tokens (by text offset).
    blank: HashMap<usize, crate::blank::Style>,
    /// Formatting directives (`fmt off` = `false`), by the offset of the token they precede.
    directives: Vec<(usize, bool)>,
    /// Tokens (by text offset) whose trailing comment is emitted by the caller, after the
    /// enclosing item, so it cannot force a value inside the item to break.
    deferred_trailing: HashSet<usize>,
}

#[derive(Clone, Copy, Default)]
struct Pad {
    before: usize,
    after: usize,
    /// Only pad when this group is broken.
    group: Option<GroupId>,
}

#[derive(Clone, Copy, PartialEq)]
enum Place {
    /// Continue the current line.
    Inline,
    /// Start a new line at the current indentation.
    NewLine,
    /// Start a new line, indented one level.
    Indented,
    /// Each child starts a new line, indented one level.
    IndentedList,
    /// Indented new line if the enclosing statement does not fit on one line.
    IndentedSoft,
}

pub(crate) struct CommentInfo {
    pub(crate) text: Rc<[u8]>,
    block: bool,
    /// Newlines between the previous comment (or the start of the trivia) and this comment.
    nl_before: usize,
    /// The comment ends the previous token's line.
    pub(crate) trailing: bool,
}

/// Leading comments of a token, classified, plus the newlines after the last comment.
/// `has_prev`: whether a token precedes `t` (comments at the start of a file are not trailing).
pub(crate) fn comments(t: &SyntaxToken, has_prev: bool) -> (Vec<CommentInfo>, usize) {
    let mut out: Vec<CommentInfo> = Vec::new();
    let mut nl = 0;
    let mut total_nl = 0;
    for piece in t.leading_trivia() {
        match piece {
            TriviaPiece::LineComment(c) | TriviaPiece::BlockComment(c) => {
                let block = matches!(piece, TriviaPiece::BlockComment(_));
                let mut text = c.as_bytes().to_vec();
                if block {
                    text = normalize_newlines(&text);
                } else {
                    while text.last().is_some_and(|b| matches!(b, b' ' | b'\t')) {
                        text.pop();
                    }
                }
                out.push(CommentInfo {
                    text: text.into(),
                    block,
                    nl_before: nl,
                    trailing: has_prev && total_nl == 0,
                });
                nl = 0;
            }
            p if p.is_newline() => {
                let n = newline_count(p);
                nl += n;
                total_nl += n;
            }
            _ => {}
        }
    }
    // A block comment on the previous token's line stays inline in front of the next token
    // unless a line break follows it.
    let mut newline_after = nl > 0;
    for c in out.iter_mut().rev() {
        if c.trailing && c.block && !newline_after {
            c.trailing = false;
        }
        newline_after |= c.nl_before > 0 || !c.block;
    }
    (out, nl)
}

/// `-- vsg-rs: fmt off` / `-- vsg-rs: fmt on` (and VSG's `-- vsg_off` / `-- vsg_on`) comments,
/// with the offset of the token that follows them, in source order.
/// Whether formatting is switched off at `offset`, given [`directives`].
pub(crate) fn disabled_at(directives: &[(usize, bool)], offset: usize) -> bool {
    let i = directives.partition_point(|(o, _)| *o <= offset);
    i > 0 && !directives[i - 1].1
}

pub(crate) fn directives(parsed: &Parsed) -> Vec<(usize, bool)> {
    let mut out = Vec::new();
    for t in parsed.tokens() {
        for piece in t.leading_trivia() {
            let TriviaPiece::LineComment(c) = piece else {
                continue;
            };
            let text = String::from_utf8_lossy(c.as_bytes()).to_ascii_lowercase();
            let body = text.trim_start_matches('-').trim();
            match body {
                "vsg-rs: fmt off" | "vsg_off" => out.push((t.text_offset(), false)),
                "vsg-rs: fmt on" | "vsg_on" => out.push((t.text_offset(), true)),
                _ => {}
            }
        }
    }
    out
}

fn newline_count(p: &TriviaPiece) -> usize {
    match p {
        TriviaPiece::CarriageReturns(n)
        | TriviaPiece::LineFeeds(n)
        | TriviaPiece::FormFeeds(n)
        | TriviaPiece::CarriageReturnLineFeeds(n)
        | TriviaPiece::VerticalTabs(n) => *n,
        _ => 0,
    }
}

/// Convert `\r\n` and lone `\r` to `\n`.
pub(crate) fn normalize_newlines(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut bytes = text.iter().peekable();
    while let Some(&b) = bytes.next() {
        if b == b'\r' {
            bytes.next_if_eq(&&b'\n');
            out.push(b'\n');
        } else {
            out.push(b);
        }
    }
    out
}

fn concat(v: Vec<Doc>) -> Doc {
    Doc::Concat(v)
}

fn kw(t: &SyntaxToken, k: Kw) -> bool {
    t.kind() == T::Keyword(k)
}

fn last_token_of(e: &SyntaxElement) -> SyntaxToken {
    match e {
        SyntaxElement::Node(n) => n.last_token(),
        SyntaxElement::Token(t) => t.clone(),
    }
}

fn first_token(e: &SyntaxElement) -> SyntaxToken {
    match e {
        SyntaxElement::Node(n) => n.first_token(),
        SyntaxElement::Token(t) => t.clone(),
    }
}

fn is_epilogue(k: N) -> bool {
    use N::*;
    matches!(
        k,
        ArchitectureEpilogue
            | BlockConfigurationEpilogue
            | BlockEpilogue
            | CaseStatementEpilogue
            | ComponentConfigurationEpilogue
            | ComponentDeclarationEpilogue
            | ConfigurationDeclarationEpilogue
            | ContextDeclarationEpilogue
            | EntityDeclarationEpilogue
            | GenerateBodyEpilogue
            | GenerateEpilogue
            | IfStatementEpilogue
            | LoopStatementEpilogue
            | PackageBodyEpilogue
            | PackageEpilogue
            | PhysicalTypeDefinitionEpilogue
            | ProcessEpilogue
            | ProtectedTypeBodyEpilogue
            | ProtectedTypeDeclarationEpilogue
            | RecordTypeDefinitionEpilogue
            | SubprogramBodyEpilogue
    )
}

fn is_item_list(k: N) -> bool {
    use N::*;
    matches!(
        k,
        ArchitectureDeclarativePart
            | ArchitectureStatementPart
            | BlockDeclarativePart
            | BlockStatementPart
            | ConfigurationDeclarativePart
            | EntityDeclarativePart
            | EntityStatementPart
            | PackageDeclarativePart
            | PackageBodyDeclarativePart
            | ProcessDeclarativePart
            | ProcessStatementPart
            | ProtectedTypeBodyDeclarativePart
            | ProtectedTypeDeclarativePart
            | SequenceOfStatements
            | SubprogramDeclarativePart
            | SubprogramStatementPart
            | EntityHeader
            | BlockHeader
            | PackageHeader
            | RecordElementDeclarations
    )
}

fn is_concurrent_statement(k: N) -> bool {
    use N::*;
    matches!(
        k,
        BlockStatement
            | ProcessStatement
            | ConcurrentProcedureCallOrComponentInstantiationStatement
            | ConcurrentAssertionStatement
            | ConcurrentSimpleSignalAssignment
            | ConcurrentConditionalSignalAssignment
            | ConcurrentSelectedSignalAssignment
            | ComponentInstantiationStatement
            | ForGenerateStatement
            | IfGenerateStatement
            | CaseGenerateStatement
    )
}

fn is_configuration_parent(k: N) -> bool {
    use N::*;
    matches!(
        k,
        ConfigurationDeclaration
            | BlockConfiguration
            | ComponentConfiguration
            | CompoundConfigurationSpecification
            | SimpleConfigurationSpecification
    )
}

fn placement(parent: N, child: N) -> Place {
    use N::*;
    match child {
        k if is_item_list(k) => Place::IndentedList,
        ContextClause if parent == ContextDeclaration => Place::IndentedList,
        k if is_epilogue(k) && parent == PhysicalTypeDefinition => Place::Indented,
        k if is_epilogue(k) => Place::NewLine,
        DesignUnit
        | DeclarationStatementSeparator
        | IfStatementElsif
        | IfStatementElse
        | IfGenerateElsif
        | IfGenerateElse => Place::NewLine,
        _ if parent == DesignUnit => Place::NewLine,
        CaseStatementAlternative | CaseGenerateAlternative => Place::Indented,
        GenericClause | PortClause if parent == ComponentDeclaration => Place::Indented,
        GenericMapAspect | PortMapAspect if !matches!(parent, GenericMap | PortMap) => {
            Place::Indented
        }
        UnitDeclarations | PrimaryUnitDeclaration | SecondaryUnitDeclaration => Place::Indented,
        ReportClause | SeverityClause if parent == Assertion => Place::Indented,
        SeverityClause | SensitivityClause | ConditionClause | TimeoutClause | AfterClause
        | FileOpenInformation => Place::IndentedSoft,
        k if parent == GenerateStatementBody && is_concurrent_statement(k) => Place::Indented,
        BlockConfigurationPreamble
        | ComponentConfigurationPreamble
        | ConfigurationDeclarationPreamble => Place::Inline,
        _ if is_configuration_parent(parent) => Place::Indented,
        _ => Place::Inline,
    }
}

fn binary_precedence(k: T) -> Option<u8> {
    Some(match k {
        T::Keyword(Kw::To | Kw::Downto) => 0,
        T::Keyword(Kw::And | Kw::Or | Kw::Nand | Kw::Nor | Kw::Xor | Kw::Xnor) => 1,
        T::EQ | T::NE | T::LT | T::LTE | T::GT | T::GTE => 2,
        T::QueEQ | T::QueNE | T::QueLT | T::QueLTE | T::QueGT | T::QueGTE => 2,
        T::Keyword(Kw::Sll | Kw::Srl | Kw::Sla | Kw::Sra | Kw::Rol | Kw::Ror) => 3,
        T::Plus | T::Minus | T::Concat => 4,
        T::Times | T::Div | T::Keyword(Kw::Mod | Kw::Rem) => 5,
        T::Pow => 6,
        _ => return None,
    })
}

fn binary_precedence_of(n: &SyntaxNode) -> Option<u8> {
    n.children_with_tokens()
        .find_map(|c| c.as_token())
        .and_then(|t| binary_precedence(t.kind()))
}

/// A value that folds well by breaking inside its own parentheses.
fn huggable(n: &SyntaxNode) -> bool {
    match n.kind() {
        N::ActualPart | N::ActualPartExpression => {
            let mut cs = n.children_with_tokens();
            match (cs.next(), cs.next()) {
                (Some(SyntaxElement::Node(c)), None) => huggable(&c),
                _ => false,
            }
        }
        N::ParenthesizedExpressionOrAggregate | N::Aggregate | N::QualifiedExpression => true,
        N::NameExpression => n
            .first_child()
            .and_then(|name| name.children().last())
            .is_some_and(|tail| tail.kind() == N::ParenthesizedName),
        _ => false,
    }
}

impl<'a> Builder<'a> {
    pub(crate) fn new(cfg: &'a FormatConfig, parsed: &'a Parsed) -> Self {
        Builder {
            cfg,
            parsed,
            utf8: parsed.is_utf8(),
            standard: parsed.standard,
            groups: 0,
            comments_done: HashSet::new(),
            item_starts: HashSet::new(),
            pads: HashMap::new(),
            directives: directives(parsed),
            blank: crate::blank::policies(parsed, &cfg.blank),
            deferred_trailing: HashSet::new(),
        }
    }

    pub(crate) fn groups(&self) -> usize {
        self.groups
    }

    fn new_group(&mut self) -> GroupId {
        self.groups += 1;
        self.groups - 1
    }

    fn group(&mut self, doc: Doc) -> Doc {
        Doc::Group {
            id: self.new_group(),
            doc: Box::new(doc),
            broken: false,
        }
    }

    fn mark_item(&mut self, e: &SyntaxElement) {
        self.item_starts.insert(first_token(e).text_offset());
    }

    // ------------------------------------------------------------ tokens and comments

    fn comment_atom(&self, c: &CommentInfo) -> Doc {
        let width = display_width(&c.text, self.utf8, 0);
        Doc::Atom {
            text: c.text.clone(),
            width,
            space: true,
        }
    }

    /// The layout of the gap before `t`: its own-line comments, the blank lines to print in front
    /// of each of them and in front of `t` (one more entry than comments), and the number of
    /// newlines after the last comment in the source.
    fn gap(&self, t: &SyntaxToken) -> (Vec<CommentInfo>, Vec<usize>, usize) {
        use crate::blank::Style;
        let blank_ok = self.item_starts.contains(&t.text_offset());
        let settings = &self.cfg.blank;
        // Formatter-off regions keep their blank lines. The gap in front of a token belongs to
        // the region of the previous token (a region starts after its `fmt off` comment).
        let disabled = self.disabled(t.text_offset());
        let gap_disabled = self
            .parsed
            .prev_token(t)
            .is_some_and(|p| self.disabled(p.text_offset()));
        let policy = self
            .blank
            .get(&t.text_offset())
            .copied()
            .filter(|_| blank_ok && !gap_disabled);
        let (cs, nl_after) = comments(t, self.parsed.prev_token(t).is_some());
        let own: Vec<CommentInfo> = cs.into_iter().filter(|c| !c.trailing).collect();
        // Blank lines in each gap: in front of each own-line comment, then in front of `t`.
        let keep = |n: usize| {
            if blank_ok {
                n.saturating_sub(1).min(settings.max_blank_lines)
            } else {
                0
            }
        };
        let mut blanks: Vec<usize> = own
            .iter()
            .map(|c| keep(c.nl_before))
            .chain([keep(nl_after)])
            .collect();
        match policy {
            Some(Style::Require) => blanks[0] = 1,
            Some(Style::NoCode) if own.is_empty() => blanks[0] = 1,
            Some(Style::NoBlank) => blanks.iter_mut().for_each(|b| *b = 0),
            _ => {}
        }
        if blank_ok && !disabled {
            for (i, c) in own.iter().enumerate() {
                match settings.pragma(&c.text) {
                    // pragma_400 (no code above) and pragma_401 (no blank line below).
                    Some(true) => {
                        if i == 0 {
                            blanks[0] = blanks[0].max(1);
                        }
                        blanks[i + 1] = 0;
                    }
                    // pragma_402 (no blank line above) and pragma_403 (blank line below).
                    Some(false) => {
                        blanks[i] = 0;
                        blanks[i + 1] = blanks[i + 1].max(1);
                    }
                    None => {}
                }
            }
        }
        (own, blanks, nl_after)
    }

    /// Own-line and inline comments before `t`, with the line breaks around them.
    /// Emitted at most once per token.
    fn leading(&mut self, t: &SyntaxToken) -> Doc {
        if !self.comments_done.insert(t.text_offset()) {
            return Doc::Nil;
        }
        let (own, blanks, nl_after) = self.gap(t);
        let blank = |n: usize| Doc::Blank(u8::try_from(n.min(255)).unwrap_or(u8::MAX));
        let mut out = Vec::new();
        let mut last_block = None;
        for (i, c) in own.iter().enumerate() {
            // A line comment always ends its line.
            if blanks[i] > 0 {
                out.push(blank(blanks[i]));
            } else if c.nl_before > 0 || last_block == Some(false) {
                out.push(Doc::Hard);
            }
            out.push(self.comment_atom(c));
            last_block = Some(c.block);
        }
        let last = blanks[own.len()];
        if last > 0 {
            out.push(blank(last));
        } else if nl_after > 0 && last_block.is_some() || last_block == Some(false) {
            out.push(Doc::Hard);
        }
        concat(out)
    }

    /// Comments at the end of `t`'s line (stored in the next token's trivia).
    fn trailing(&self, t: &SyntaxToken) -> Doc {
        if self.deferred_trailing.contains(&t.text_offset()) {
            return Doc::Nil;
        }
        self.trailing_now(t)
    }

    fn trailing_now(&self, t: &SyntaxToken) -> Doc {
        let Some(next) = self.parsed.next_token(t) else {
            return Doc::Nil;
        };
        let mut suffix = Vec::new();
        for c in comments(next, true).0.iter().filter(|c| c.trailing) {
            suffix.extend(std::iter::repeat_n(b' ', self.cfg.comment_spaces));
            suffix.extend_from_slice(&c.text);
        }
        if suffix.is_empty() {
            Doc::Nil
        } else {
            concat(vec![Doc::Suffix(suffix.into()), Doc::BreakParent])
        }
    }

    pub(crate) fn token_text(&self, t: &SyntaxToken) -> Rc<[u8]> {
        let bytes = t.text().as_bytes();
        let case = match t.kind() {
            T::Keyword(_)
                if !(self.cfg.keyword_case_overrides.is_empty()
                    && self.cfg.keyword_case_in.is_empty()) =>
            {
                self.keyword_case(t, &String::from_utf8_lossy(&bytes.to_ascii_lowercase()))
            }
            _ => self.cfg.keyword_case,
        };
        match (t.kind(), case) {
            (T::Keyword(_), KeywordCase::Lower) => bytes.to_ascii_lowercase().into(),
            (T::Keyword(_), KeywordCase::Upper) => bytes.to_ascii_uppercase().into(),
            _ => bytes.into(),
        }
    }

    /// The case of keyword `word`: from the innermost construct with a rule for it, else from
    /// a rule for it everywhere, else the keyword default.
    fn keyword_case(&self, t: &SyntaxToken, word: &str) -> KeywordCase {
        if let Some(entries) = self.cfg.keyword_case_in.get(word) {
            let mut node = Some(t.parent());
            while let Some(n) = node {
                let kind = construct_kind(&n);
                if let Some((_, case)) = entries.iter().find(|(k, _)| *k == kind) {
                    return *case;
                }
                node = n.parent();
            }
        }
        self.cfg
            .keyword_case_overrides
            .get(word)
            .copied()
            .unwrap_or(self.cfg.keyword_case)
    }

    fn atom(&self, t: &SyntaxToken) -> Doc {
        let text = self.token_text(t);
        if text.is_empty() {
            return Doc::Nil;
        }
        let width = display_width(&text, self.utf8, 0);
        let space = self.space_before(t);
        if space
            && !self.cfg.spacing.is_empty()
            && let Some(prev) = self.parsed.prev_token(t)
            && let Some(spaces) =
                crate::layout::spacing_for(self.parsed, &self.cfg.spacing, prev, t)
        {
            let n = match spaces {
                crate::layout::Spaces::Exact(n) => n,
                // Aligned tokens get their spaces from the alignment, which must stay stable.
                crate::layout::Spaces::AtLeast(n) if self.padded(prev, t) => n,
                crate::layout::Spaces::AtLeast(n) => {
                    let gap = &self.parsed.source()[prev.text_range().end..t.text_offset()];
                    if gap.iter().all(|b| matches!(b, b' ' | b'\t')) {
                        gap.len().max(n)
                    } else {
                        n
                    }
                }
            };
            // Zero spaces only where the tokens stay apart without one.
            let n = if n == 0 && requires_separator(prev.token(), t.token(), self.standard) {
                1
            } else {
                n
            };
            return concat(vec![
                Doc::Space(n),
                Doc::Atom {
                    text,
                    width,
                    space: false,
                },
            ]);
        }
        Doc::Atom { text, width, space }
    }

    /// Whether alignment adds spaces between `prev` and `t`.
    fn padded(&self, prev: &SyntaxToken, t: &SyntaxToken) -> bool {
        self.pads
            .get(&prev.text_offset())
            .is_some_and(|p| p.after > 0)
            || self
                .pads
                .get(&t.text_offset())
                .is_some_and(|p| p.before > 0)
    }

    fn pad(n: usize, group: Option<GroupId>) -> Doc {
        if n == 0 {
            return Doc::Nil;
        }
        let atom = Doc::Atom {
            text: vec![b' '; n].into(),
            width: n,
            space: false,
        };
        match group {
            Some(id) => Doc::IfBreak {
                id,
                broken: Box::new(atom),
                flat: Box::new(Doc::Nil),
            },
            None => atom,
        }
    }

    fn tok(&mut self, t: &SyntaxToken) -> Doc {
        let pad = self.pads.get(&t.text_offset()).copied().unwrap_or_default();
        concat(vec![
            self.leading(t),
            Self::pad(pad.before, pad.group),
            self.atom(t),
            Self::pad(pad.after, pad.group),
            self.trailing(t),
        ])
    }

    fn space_before(&self, t: &SyntaxToken) -> bool {
        let Some(p) = self.parsed.prev_token(t) else {
            return false;
        };
        want_space(p, t) || requires_separator(p.token(), t.token(), self.standard)
    }

    // ------------------------------------------------------------ nodes

    fn elem(&mut self, e: &SyntaxElement) -> Doc {
        match e {
            SyntaxElement::Node(n) => self.node(n),
            SyntaxElement::Token(t) => self.tok(t),
        }
    }

    pub(crate) fn node(&mut self, n: &SyntaxNode) -> Doc {
        if n.kind() == N::DesignFile {
            // The design file marks its items (and their blank lines) itself.
            return self.node_body(n);
        }
        let first = n.first_token();
        if self.item_starts.contains(&first.text_offset()) && self.disabled(first.text_offset()) {
            return self.verbatim(n);
        }
        // Comments in front of a node stay outside of the groups built for it, so their line
        // breaks do not force the node itself to break.
        let lead = self.leading(&first);
        let doc = self.node_body(n);
        concat(vec![lead, doc])
    }

    /// Whether formatting is switched off at a token.
    fn disabled(&self, offset: usize) -> bool {
        disabled_at(&self.directives, offset)
    }

    /// An item inside a `fmt off` region, printed as written (only its first line is
    /// re-indented).
    fn verbatim(&mut self, n: &SyntaxNode) -> Doc {
        let (first, last) = (n.first_token(), n.last_token());
        let lead = self.leading(&first);
        let text =
            normalize_newlines(&self.parsed.source()[first.text_offset()..last.text_range().end]);
        let width = display_width(&text, self.utf8, 0);
        let atom = Doc::Atom {
            text: text.into(),
            width,
            space: self.space_before(&first),
        };
        concat(vec![lead, atom, self.trailing(&last)])
    }

    fn node_body(&mut self, n: &SyntaxNode) -> Doc {
        use N::*;
        match n.kind() {
            DesignFile => self.design_file(n),
            ContextClause => self.context_clause(n),
            BinaryExpression => self.binary(n),
            GenericClause | PortClause | GenericMapAspect | PortMapAspect => self.broken_parens(n),
            RecordElementDeclarations => {
                let level = self
                    .cfg
                    .indent_policy
                    .level(N::RecordTypeDefinition, Part::Body);
                self.items(n, level)
            }
            ConditionalWaveforms | ConditionalExpressions => {
                let doc = self.children(n);
                self.group(Doc::align(doc))
            }
            SelectedWaveforms | SelectedExpressions => self.selected(n),
            // `[t, t return t]` moves to a continuation line as a whole, then folds inside.
            Signature => {
                let body = self.children(n);
                self.group(Doc::indent(concat(vec![Doc::Line, Doc::align(body)])))
            }
            WhenWaveform
            | ElseWhenWaveform
            | ElseWaveform
            | WhenExpression
            | ElseWhenExpression
            | ElseExpression
            | SelectedWaveformItem
            | SelectedExpressionItem => self.branch(n),
            IdentifierList
            | WaveformElements
            | SensitivityList
            | NameList
            | LogicalNameList
            | Choices
            | EntityDesignatorList
            | InstantiationListList
            | SignalListList
            | TypeMarkList
            | VerificationUnitList
            | ExpressionList => self.flat_list(n),
            InitialValue => {
                let value = n.children().next();
                // With a value that folds inside itself, `:=` stays on the line and only the
                // value may move.
                if let Some(value) = value.filter(huggable) {
                    let assign = n.first_token();
                    let assign = self.tok(&assign);
                    let value = self.node(&value);
                    concat(vec![assign, self.right_hand_side(value, true)])
                } else {
                    let doc = self.block(n);
                    self.right_hand_side(doc, false)
                }
            }
            SubtypeIndication
                if n.parent().is_some_and(|p| {
                    matches!(
                        p.kind(),
                        SignalDeclaration
                            | ConstantDeclaration
                            | VariableDeclaration
                            | FileDeclaration
                    )
                }) =>
            {
                // A long subtype may move after the `:`.
                let doc = self.block(n);
                self.right_hand_side(doc, true)
            }
            AssociationElement | ElementAssociation => {
                let mut cs = n.children_with_tokens();
                let head = cs.next();
                let named = head
                    .as_ref()
                    .and_then(SyntaxElement::as_node)
                    .is_some_and(|h| matches!(h.kind(), Formal | ElementChoices));
                let Some(head) = head.filter(|_| named) else {
                    return self.block(n);
                };
                let rest: Vec<SyntaxElement> = cs.collect();
                let hug = rest
                    .iter()
                    .filter_map(SyntaxElement::as_node)
                    .any(|v| huggable(&v));
                let head = self.elem(&head);
                let rest = rest.iter().map(|c| self.elem(c)).collect();
                concat(vec![head, self.right_hand_side(concat(rest), hug)])
            }
            _ => self.block(n),
        }
    }

    /// The value after `:=` or `=>`. It moves to an indented continuation line when it does
    /// not fit; a value that folds inside its own parentheses stays on the line when its first
    /// line fits.
    fn right_hand_side(&mut self, value: Doc, huggable: bool) -> Doc {
        if huggable {
            Doc::Hug {
                value: Box::new(value),
                forced: false,
            }
        } else {
            self.group(Doc::indent(concat(vec![Doc::Line, value])))
        }
    }

    fn children(&mut self, n: &SyntaxNode) -> Doc {
        let docs = n.children_with_tokens().map(|c| self.elem(&c)).collect();
        concat(docs)
    }

    fn design_file(&mut self, n: &SyntaxNode) -> Doc {
        let mut out = Vec::new();
        for c in n.children_with_tokens() {
            self.mark_item(&c);
            out.push(Doc::Hard);
            out.push(self.elem(&c));
        }
        concat(out)
    }

    /// Context items each on their own line; `use` clauses following a `library` clause are
    /// indented one level.
    fn context_clause(&mut self, n: &SyntaxNode) -> Doc {
        let mut out = Vec::new();
        let mut seen_library = false;
        for c in n.children_with_tokens() {
            self.mark_item(&c);
            let kind = c.as_node().map(|n| n.kind());
            seen_library |= kind == Some(N::LibraryClause);
            let doc = concat(vec![Doc::Hard, self.elem(&c)]);
            out.push(if seen_library && kind == Some(N::UseClauseContextItem) {
                Doc::indent_n(self.cfg.indent_policy.use_after_library.unwrap_or(1), doc)
            } else {
                doc
            });
        }
        concat(out)
    }

    /// Generic structural layout: children placed according to [`placement`], parenthesized
    /// child sequences folded as groups.
    fn block(&mut self, n: &SyntaxNode) -> Doc {
        let children: Vec<SyntaxElement> = n.children_with_tokens().collect();
        let mut out = Vec::new();
        let mut soft = false;
        let mut prev = (Place::Inline, false); // (placement, was a label)
        let construct = indent_construct(n);
        // Parts of a generate body are statements; elsewhere, those after `begin`.
        let mut after_begin = n.kind() == N::GenerateStatementBody;
        let mut body_level = 1;
        let mut i = 0;
        while i < children.len() {
            let c = match &children[i] {
                SyntaxElement::Token(t) => {
                    match (t.kind(), matching_paren(&children, i)) {
                        (T::LeftPar, Some(close)) => {
                            out.push(self.parens(n.kind(), &children[i..=close]));
                            i = close + 1;
                        }
                        _ if is_assignment_operator(n.kind(), t) => {
                            // The value stays after the operator when its first line fits,
                            // otherwise it moves to an indented continuation line.
                            out.push(self.tok(t));
                            let end = children.len() - 1;
                            let value: Vec<Doc> =
                                children[i + 1..end].iter().map(|c| self.elem(c)).collect();
                            let value = concat(value);
                            let moved = Doc::indent(concat(vec![Doc::Line, value.clone()]));
                            out.push(Doc::Choice {
                                flat: Box::new(value),
                                broken: Box::new(moved),
                            });
                            i = end;
                        }
                        _ if soft_break_before(n.kind(), t) => {
                            soft = true;
                            let rest: Vec<Doc> =
                                children[i..].iter().map(|c| self.elem(c)).collect();
                            out.push(Doc::indent(concat(vec![Doc::Line, concat(rest)])));
                            i = children.len();
                        }
                        _ => {
                            out.push(self.tok(t));
                            i += 1;
                        }
                    }
                    prev = (Place::Inline, false);
                    continue;
                }
                SyntaxElement::Node(c) => c.clone(),
            };
            let mut place = placement(n.kind(), c.kind());
            if prev.1 && place == Place::NewLine {
                place = Place::Inline;
            }
            // The first clause of a wait statement stays on the `wait` line.
            if n.kind() == N::WaitStatement && i > 0 && children[i - 1].as_token().is_some() {
                place = Place::Inline;
            }
            match place {
                Place::Inline => out.push(self.node(&c)),
                Place::NewLine => {
                    self.mark_item(&children[i]);
                    if matches!(prev.0, Place::Indented | Place::IndentedList) {
                        // Comments before a closing `end` belong to the body above it.
                        if let Some(t) = Some(c.first_token()).filter(|t| kw(t, Kw::End)) {
                            out.push(Doc::indent_n(body_level, self.leading(&t)));
                        }
                    }
                    let part = match c.kind() {
                        k if is_epilogue(k) => Some(Part::End),
                        N::DeclarationStatementSeparator => {
                            after_begin = true;
                            Some(Part::Begin)
                        }
                        N::IfStatementElsif
                        | N::IfStatementElse
                        | N::IfGenerateElsif
                        | N::IfGenerateElse => Some(Part::Branch),
                        _ => None,
                    };
                    let level = part.map_or(0, |p| self.cfg.indent_policy.level(construct, p));
                    let doc = self.node(&c);
                    out.push(Doc::indent_n(level, concat(vec![Doc::Hard, doc])));
                }
                Place::Indented | Place::IndentedList => {
                    let part = if after_begin {
                        Part::Statements
                    } else {
                        Part::Body
                    };
                    body_level = self.cfg.indent_policy.level(construct, part);
                    if place == Place::IndentedList {
                        out.push(self.items(&c, body_level));
                    } else {
                        self.mark_item(&children[i]);
                        let doc = self.node(&c);
                        out.push(Doc::indent_n(body_level, concat(vec![Doc::Hard, doc])));
                    }
                }
                Place::IndentedSoft => {
                    soft = true;
                    let doc = self.node(&c);
                    out.push(Doc::indent(concat(vec![Doc::Line, doc])));
                }
            }
            prev = (place, c.kind() == N::StmtLabel);
            i += 1;
        }
        if soft {
            self.group(concat(out))
        } else {
            concat(out)
        }
    }

    /// Child nodes of `n`, each on a new line, indented `levels` levels.
    fn items(&mut self, n: &SyntaxNode, levels: usize) -> Doc {
        if n.kind() == N::RecordElementDeclarations {
            let items: Vec<SyntaxElement> = n.children_with_tokens().collect();
            self.align_items(&items, None);
        }
        for c in n.children_with_tokens().filter(SyntaxElement::is_node) {
            self.mark_item(&c);
        }
        self.align_statements(n);
        let mut out = Vec::new();
        for c in n.children_with_tokens() {
            if c.is_node() {
                self.mark_item(&c);
                out.push(Doc::Hard);
            }
            out.push(self.elem(&c));
        }
        Doc::indent_n(levels, concat(out))
    }

    /// `( ... )`: a group that is either flat or has one element per line. A single
    /// parenthesized expression instead folds inside, aligned after the parenthesis.
    fn parens(&mut self, parent: N, seq: &[SyntaxElement]) -> Doc {
        let (Some(open), Some(close)) = (seq[0].as_token(), seq[seq.len() - 1].as_token()) else {
            unreachable!("parenthesized sequences start and end with tokens")
        };
        let items = list_items(&seq[1..seq.len() - 1]);
        let lead = self.leading(&open);
        let id = self.new_group();
        let expression = matches!(
            parent,
            N::ParenthesizedExpressionOrAggregate | N::ParenthesizedCondition | N::PathnameElement
        ) && items.len() == 1
            && !is_named_association(&items[0]);
        let doc = if expression {
            let body = items[0].iter().map(|e| self.elem(e)).collect();
            concat(vec![
                self.tok(&open),
                Doc::align(concat(body)),
                self.tok(&close),
            ])
        } else {
            self.align_items(&first_elements(&items), Some(id));
            let fill = matches!(parent, N::ParenthesizedExpressionOrAggregate | N::Aggregate)
                && items.len() > 1
                && items.iter().all(|i| is_simple_item(i));
            let mut body = Vec::new();
            for (k, item) in items.iter().enumerate() {
                if k > 0 {
                    body.push(Doc::Line);
                }
                body.push(concat(item.iter().map(|e| self.elem(e)).collect()));
            }
            let body = if fill { Doc::Fill(body) } else { concat(body) };
            let body = vec![Doc::Line, body, self.leading(&close)];
            concat(vec![
                self.tok(&open),
                Doc::indent(concat(body)),
                Doc::Line,
                self.tok(&close),
            ])
        };
        concat(vec![
            lead,
            Doc::Group {
                id,
                doc: Box::new(doc),
                broken: false,
            },
        ])
    }

    /// Interface clauses and map aspects: always one element per line.
    fn broken_parens(&mut self, n: &SyntaxNode) -> Doc {
        let children: Vec<SyntaxElement> = n.children_with_tokens().collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < children.len() {
            let close = match children[i].as_token() {
                Some(t) if t.kind() == T::LeftPar => matching_paren(&children, i),
                _ => None,
            };
            let Some(close) = close else {
                out.push(self.elem(&children[i]));
                i += 1;
                continue;
            };
            let items = list_items(&children[i + 1..close]);
            self.align_items(&first_elements(&items), None);
            let mut body = Vec::new();
            for item in &items {
                self.mark_item(&item[0]);
                body.push(Doc::Hard);
                // The comment after the last element (before `)`) ends the item's line.
                let last = item.last().map(last_token_of);
                if let Some(last) = &last {
                    self.deferred_trailing.insert(last.text_offset());
                }
                body.extend(item.iter().map(|e| self.elem(e)));
                if let Some(last) = last {
                    self.deferred_trailing.remove(&last.text_offset());
                    body.push(self.trailing_now(&last));
                }
            }
            let close_tok = children[close].as_token().expect("matched parenthesis");
            body.push(self.leading(&close_tok));
            let policy = &self.cfg.indent_policy;
            let (body_level, close_level) = (
                policy.level(n.kind(), Part::Body),
                policy.level(n.kind(), Part::End),
            );
            out.push(self.elem(&children[i]));
            out.push(Doc::indent_n(body_level, concat(body)));
            if self.cfg.close_paren_same_line.contains(&n.kind()) {
                out.push(self.tok(&close_tok));
            } else {
                let closing = concat(vec![Doc::Hard, self.tok(&close_tok)]);
                out.push(Doc::indent_n(close_level, closing));
            }
            i = close + 1;
        }
        concat(out)
    }

    /// Whether the `:` or `=>` of a list element is aligned, by the VSG rule of its construct,
    /// and whether that rule wants the narrowest column (`compact_alignment`).
    fn alignment_of(&self, n: &SyntaxNode, sep: &SyntaxToken) -> (bool, bool) {
        let align = &self.cfg.align;
        if sep.kind() == T::RightArrow {
            return (align.map_arrows, align.map_arrows_compact);
        }
        let mut node = n.parent();
        while let Some(p) = node {
            match p.kind() {
                N::ComponentDeclaration => {
                    return (align.component_colons, align.component_colons_compact);
                }
                N::EntityDeclaration | N::BlockStatement => {
                    return (align.interface_colons, align.interface_colons_compact);
                }
                N::FunctionSpecification | N::ProcedureSpecification => {
                    return (align.parameter_colons, align.parameter_colons_compact);
                }
                N::RecordTypeDefinition => return (true, true),
                _ => node = p.parent(),
            }
        }
        (true, true)
    }

    /// Request alignment padding for list elements that are printed one per line: `:` in
    /// interface and record element declarations, `=>` in named associations, and port modes
    /// padded to a common width.
    fn align_items(&mut self, items: &[SyntaxElement], group: Option<GroupId>) {
        let mut targets = Vec::new();
        let mut compact = true;
        for n in items
            .iter()
            .filter_map(vhdl_syntax::syntax::child::Child::as_node)
        {
            let sep = match n.kind() {
                N::InterfaceObjectDeclaration
                | N::InterfaceFileDeclaration
                | N::ElementDeclaration => direct_token(&n, |k| k == T::Colon),
                N::AssociationElement | N::ElementAssociation => n
                    .first_child()
                    .filter(|c| matches!(c.kind(), N::Formal | N::ElementChoices))
                    .map(|c| c.last_token())
                    .filter(|t| t.kind() == T::RightArrow),
                _ => None,
            };
            let Some(sep) = sep else { continue };
            // Very wide prefixes (long identifier lists) are left unaligned rather than pushing
            // every other element to the right.
            let (enabled, wants_compact) = self.alignment_of(&n, &sep);
            if let Some(width) = self
                .flat_width_before(&n, &sep)
                .filter(|w| *w <= self.cfg.width / 2 && enabled)
            {
                compact &= wants_compact;
                // What the source itself puts before the separator, measured from the start of
                // the element so that it can be compared with the flat width.
                let start = self.parsed.line_col(n.first_token().text_offset()).1;
                let at = self.parsed.line_col(sep.text_offset()).1;
                targets.push((sep, width, at.saturating_sub(start)));
            }
            let mode = direct_token(&n, |k| {
                matches!(
                    k,
                    T::Keyword(Kw::In | Kw::Out | Kw::Inout | Kw::Buffer | Kw::Linkage)
                )
            });
            if let Some(mode) = mode.filter(|_| n.kind() == N::InterfaceObjectDeclaration) {
                let pad = self.pads.entry(mode.text_offset()).or_default();
                let configured = match mode.kind() {
                    T::Keyword(Kw::In) => Some(self.cfg.mode_spacing[0]),
                    T::Keyword(Kw::Out) => Some(self.cfg.mode_spacing[1]),
                    T::Keyword(Kw::Inout) => Some(self.cfg.mode_spacing[2]),
                    _ => None,
                };
                // One space is printed anyway; the pads add the rest.
                pad.after = configured.map_or_else(
                    || MODE_WIDTH.saturating_sub(mode.text().len()),
                    |(_, after)| after - 1,
                );
                pad.before = configured.map_or(0, |(before, _)| before - 1);
                pad.group = group;
            }
        }
        // `:=` of interface elements, aligned after the `:` column has been decided.
        self.align_interface_defaults(items, group, &targets);
        let Some(mut max) = targets.iter().map(|(_, w, _)| *w).max() else {
            return;
        };
        if !compact && targets.len() > 1 {
            // The list may already agree on a wider column; without `compact_alignment` that is
            // a choice to keep rather than an error to correct.
            // One space is printed anyway, so the column that reproduces the source spacing is
            // one less than the distance from the start of the element to the separator.
            let first = targets[0].2.saturating_sub(1);
            if targets
                .iter()
                .all(|(_, _, gap)| gap.saturating_sub(1) == first)
                && first >= max
                && first <= self.cfg.width / 2
            {
                max = first;
            }
        }
        for (sep, width, _) in targets {
            let pad = self.pads.entry(sep.text_offset()).or_default();
            pad.before = max - width;
            pad.group = group;
        }
    }

    /// `:=` of interface elements in one list (`entity_018`), aligned among themselves. The `:`
    /// padding decided by the caller shifts them all equally, so it does not change which column
    /// they share.
    fn align_interface_defaults(
        &mut self,
        items: &[SyntaxElement],
        group: Option<GroupId>,
        colons: &[(SyntaxToken, usize, usize)],
    ) {
        if !self.cfg.align.interface_assignments || colons.len() < 2 {
            return;
        }
        let mut targets: Vec<(SyntaxToken, usize, usize)> = Vec::new();
        for n in items
            .iter()
            .filter_map(vhdl_syntax::syntax::child::Child::as_node)
            .filter(|n| n.kind() == N::InterfaceObjectDeclaration)
        {
            let Some(value) = n.children().find(|c| c.kind() == N::InitialValue) else {
                continue;
            };
            let assign = value.first_token();
            let Some(colon) = direct_token(&n, |k| k == T::Colon) else {
                continue;
            };
            // Width of what stands between the `:` and the `:=`, which is what has to line up.
            let Some(width) = self.flat_width_between(&colon, &assign) else {
                continue;
            };
            let start = self.parsed.line_col(colon.text_offset()).1;
            let at = self.parsed.line_col(assign.text_offset()).1;
            targets.push((assign, width, at.saturating_sub(start).saturating_sub(1)));
        }
        if targets.len() < 2 {
            return;
        }
        let mut max = targets.iter().map(|(_, w, _)| *w).max().unwrap_or(0);
        if !self.cfg.align.interface_assignments_compact {
            let first = targets[0].2;
            if targets.iter().all(|(_, _, gap)| *gap == first) && first >= max {
                max = first;
            }
        }
        for (assign, width, _) in targets {
            let pad = self.pads.entry(assign.text_offset()).or_default();
            pad.before = max - width;
            pad.group = group;
        }
    }

    /// Flat width of what stands between two tokens of one element, spaced as it will be
    /// printed (the same measure as `flat_width_before`).
    fn flat_width_between(&self, from: &SyntaxToken, to: &SyntaxToken) -> Option<usize> {
        let start = self.parsed.token_index(from)?;
        let end = self.parsed.token_index(to)?;
        let mut width = 0;
        for i in (start + 1)..end {
            let t = self.parsed.tokens().get(i)?;
            if t.leading_trivia().contains_comments() {
                return None;
            }
            if i > start + 1 && self.space_before(t) {
                width += 1;
            }
            width += display_width(&self.token_text(t), self.utf8, 0);
        }
        Some(width)
    }

    /// VSG alignment of consecutive declarations (names, `:` and `:=`) and assignments
    /// (`<=` and `:=`) among the children of an item list.
    fn align_statements(&mut self, n: &SyntaxNode) {
        let align = self.cfg.align.clone();
        let (family, declarations) = match n.kind() {
            k if is_declarative_part(k) => (align.declaration_colons, true),
            N::ArchitectureStatementPart | N::BlockStatementPart => {
                (align.concurrent_assignments, false)
            }
            N::ProcessStatementPart | N::SubprogramStatementPart | N::SequenceOfStatements => {
                (align.sequential_assignments, false)
            }
            _ => return,
        };
        let mut groups: Vec<Vec<SyntaxNode>> = vec![Vec::new()];
        for c in n.children() {
            let first = c.first_token();
            let (own, blanks, _) = self.gap(&first);
            // Group boundaries follow the declarations' own families for names and `:=`; the
            // colon family decides for declarations.
            let ends = (family.blank_line_ends_group && blanks.iter().any(|b| *b > 0))
                || (family.comment_line_ends_group && !own.is_empty())
                || self.disabled(first.text_offset());
            if ends {
                groups.push(Vec::new());
            }
            let alignable = if declarations {
                is_alignable_declaration(c.kind())
            } else {
                assignment_operator(&c).is_some()
            };
            if alignable && !self.disabled(first.text_offset()) {
                groups.last_mut().expect("non-empty").push(c);
            } else {
                groups.push(Vec::new());
            }
        }
        for group in groups.iter().filter(|g| g.len() > 1) {
            if declarations {
                self.align_declarations(group, &align);
            } else if family.enabled {
                let ops: Vec<(SyntaxNode, SyntaxToken)> = group
                    .iter()
                    .filter_map(|c| assignment_operator(c).map(|op| (c.clone(), op)))
                    .collect();
                self.pad_to_common_column(&ops, &HashMap::new(), family.compact);
            }
        }
    }

    fn align_declarations(&mut self, group: &[SyntaxNode], align: &crate::align::AlignSettings) {
        // Each column is padded in turn; the earlier columns shift the later ones to the right.
        type Column = (bool, bool, fn(&SyntaxNode) -> Option<SyntaxToken>);
        let columns: [Column; 3] = [
            (
                align.declaration_names.enabled,
                align.declaration_names.compact,
                declared_name,
            ),
            (
                align.declaration_colons.enabled,
                align.declaration_colons.compact,
                |d| direct_token(d, |k| k == T::Colon),
            ),
            (
                align.declaration_assignments.enabled,
                align.declaration_assignments.compact,
                |d| {
                    d.children()
                        .find(|c| c.kind() == N::InitialValue)
                        .map(|v| v.first_token())
                },
            ),
        ];
        let mut shift: HashMap<usize, usize> = HashMap::new();
        for (enabled, compact, token_of) in columns {
            if !enabled {
                continue;
            }
            let targets: Vec<(SyntaxNode, SyntaxToken)> = group
                .iter()
                .filter_map(|d| token_of(d).map(|t| (d.clone(), t)))
                .collect();
            for (d, pad) in self.pad_to_common_column(&targets, &shift, compact) {
                *shift.entry(d).or_default() += pad;
            }
        }
    }

    /// Pad in front of each token so that all start in the same column (the rightmost one).
    /// `shift` holds padding already requested earlier in each node (by node offset). Returns
    /// the padding added per node offset.
    fn pad_to_common_column(
        &mut self,
        targets: &[(SyntaxNode, SyntaxToken)],
        shift: &HashMap<usize, usize>,
        compact: bool,
    ) -> Vec<(usize, usize)> {
        let widths: Vec<(usize, &SyntaxToken, usize)> = targets
            .iter()
            .filter_map(|(n, t)| {
                let w = self.flat_width_before(n, t)? + shift.get(&n.offset()).unwrap_or(&0);
                (w <= self.cfg.width / 2).then_some((n.offset(), t, w))
            })
            .collect();
        if widths.len() < 2 {
            return Vec::new();
        }
        let mut max = widths.iter().map(|(_, _, w)| *w).max().unwrap_or(0);
        if !compact {
            // The group may already agree on a wider column, which without `compact_alignment`
            // is a choice to respect rather than an error to correct. Measured from the start of
            // each declaration, and less the one space that is printed anyway, so that it can be
            // compared with the flat widths.
            let gaps: Vec<usize> = targets
                .iter()
                .map(|(n, t)| {
                    let start = self.parsed.line_col(n.first_token().text_offset()).1;
                    let at = self.parsed.line_col(t.text_offset()).1;
                    at.saturating_sub(start).saturating_sub(1)
                })
                .collect();
            if let Some(first) = gaps.first()
                && gaps.iter().all(|gap| gap == first)
                && *first >= max
                && *first <= self.cfg.width / 2
            {
                max = *first;
            }
        }
        let mut added = Vec::new();
        for (node, t, w) in widths {
            let pad = self.pads.entry(t.text_offset()).or_default();
            pad.before += max - w;
            added.push((node, max - w));
        }
        added
    }

    /// Flat width of the tokens of `n` before `sep`, or `None` if comments intervene.
    fn flat_width_before(&self, n: &SyntaxNode, sep: &SyntaxToken) -> Option<usize> {
        let start = self.parsed.token_index(&n.first_token())?;
        let mut width = 0;
        for (k, tok) in self.parsed.tokens()[start..].iter().enumerate() {
            if k > 0 && tok.leading_trivia().contains_comments() {
                return None;
            }
            if tok.text_offset() == sep.text_offset() {
                return Some(width);
            }
            if k > 0 && self.space_before(tok) {
                width += 1;
            }
            width += display_width(&self.token_text(tok), self.utf8, 0);
        }
        None
    }

    /// A separated list that is not parenthesized: continuation lines align with its start.
    fn flat_list(&mut self, n: &SyntaxNode) -> Doc {
        if n.children_with_tokens().nth(1).is_none() {
            return self.children(n);
        }
        let mut out = Vec::new();
        for c in n.children_with_tokens() {
            let sep = c.as_token().map(|t| t.kind());
            if sep == Some(T::Bar) {
                out.push(Doc::Line);
            }
            out.push(self.elem(&c));
            if sep == Some(T::Comma) {
                out.push(Doc::Line);
            }
        }
        self.group(Doc::align(concat(out)))
    }

    /// Chains of binary operators of equal precedence fold before each operator, with
    /// continuation lines aligned to the first operand.
    fn binary(&mut self, n: &SyntaxNode) -> Doc {
        let prec = binary_precedence_of(n);
        let mut chain = vec![n.clone()];
        while let Some(lhs) = chain
            .last()
            .and_then(vhdl_syntax::syntax::SyntaxNode::first_child)
        {
            if prec.is_none()
                || lhs.kind() != N::BinaryExpression
                || binary_precedence_of(&lhs) != prec
            {
                break;
            }
            chain.push(lhs);
        }
        let mut out = Vec::new();
        for (depth, b) in chain.iter().rev().enumerate() {
            for (j, c) in b.children_with_tokens().enumerate() {
                if depth > 0 && j == 0 {
                    continue; // the nested chain element, already emitted
                }
                if c.as_token()
                    .is_some_and(|t| binary_precedence(t.kind()).is_some())
                {
                    out.push(Doc::Line);
                }
                out.push(self.elem(&c));
            }
        }
        self.group(Doc::align(concat(out)))
    }

    /// A branch of a conditional or selected assignment. A leading `else` ends the previous
    /// line; the `when` part folds onto an indented continuation line.
    fn branch(&mut self, n: &SyntaxNode) -> Doc {
        let mut head = Vec::new();
        let mut value = Vec::new();
        let mut when = Vec::new();
        for (i, c) in n.children_with_tokens().enumerate() {
            match c.as_token() {
                Some(t) if i == 0 && kw(&t, Kw::Else) => {
                    head.push(self.tok(&t));
                    head.push(Doc::Hard);
                }
                Some(t) if kw(&t, Kw::When) => {
                    when.push(Doc::Line);
                    when.push(self.tok(&t));
                }
                _ if !when.is_empty() => when.push(self.elem(&c)),
                _ => value.push(self.elem(&c)),
            }
        }
        if !when.is_empty() {
            value.push(Doc::indent(concat(when)));
        }
        let value = self.group(concat(value));
        concat(vec![concat(head), value])
    }

    /// Selected assignment alternatives: always one per line, indented.
    fn selected(&mut self, n: &SyntaxNode) -> Doc {
        let mut out = Vec::new();
        for c in n.children_with_tokens() {
            if c.is_node() {
                self.mark_item(&c);
                out.push(Doc::Hard);
            }
            out.push(self.elem(&c));
        }
        Doc::indent(concat(out))
    }
}

/// Width that port modes are padded to (the width of `inout`), matching VSG's default layout.
const MODE_WIDTH: usize = 5;

/// The kind of a construct for keyword rules; subprogram bodies and declarations count as
/// their function or procedure specification.
pub(crate) fn construct_kind(n: &SyntaxNode) -> N {
    let spec = match n.kind() {
        N::SubprogramBody => n
            .children()
            .find(|c| c.kind() == N::SubprogramBodyPreamble)
            .and_then(|p| p.first_child()),
        N::SubprogramDeclaration => n.first_child(),
        _ => None,
    };
    spec.map_or(n.kind(), |s| s.kind())
}

/// The construct whose `indent.tokens` settings apply to the children of `n`.
fn indent_construct(n: &SyntaxNode) -> N {
    if matches!(
        n.kind(),
        N::GenerateStatementBody | N::GenerateBodyDeclarations
    ) {
        n.ancestors()
            .map(|a| a.kind())
            .find(|k| !matches!(k, N::GenerateStatementBody | N::GenerateBodyDeclarations))
            .unwrap_or(n.kind())
    } else {
        n.kind()
    }
}

fn is_declarative_part(k: N) -> bool {
    use N::*;
    matches!(
        k,
        ArchitectureDeclarativePart
            | BlockDeclarativePart
            | ProcessDeclarativePart
            | PackageDeclarativePart
            | PackageBodyDeclarativePart
            | SubprogramDeclarativePart
            | ProtectedTypeBodyDeclarativePart
            | ProtectedTypeDeclarativePart
            | EntityDeclarativePart
    )
}

fn is_alignable_declaration(k: N) -> bool {
    use N::*;
    matches!(
        k,
        SignalDeclaration
            | ConstantDeclaration
            | VariableDeclaration
            | FileDeclaration
            | AliasDeclaration
            | FullTypeDeclaration
            | IncompleteTypeDeclaration
            | SubtypeDeclaration
            | AttributeDeclaration
    )
}

/// The first declared identifier of a declaration.
fn declared_name(d: &SyntaxNode) -> Option<SyntaxToken> {
    d.children()
        .find(|c| c.kind() == N::IdentifierList)
        .map(|l| l.first_token())
        .or_else(|| direct_token(d, |k| k == T::Identifier))
}

/// The `<=` or `:=` of a simple or conditional assignment statement.
fn assignment_operator(n: &SyntaxNode) -> Option<SyntaxToken> {
    use N::*;
    let op = match n.kind() {
        ConcurrentSimpleSignalAssignment
        | ConcurrentConditionalSignalAssignment
        | SimpleWaveformAssignment
        | ConditionalWaveformAssignment
        | SimpleForceAssignment
        | ConditionalForceAssignment
        | SimpleReleaseAssignment => T::LTE,
        SimpleVariableAssignment | ConditionalVariableAssignment => T::ColonEq,
        _ => return None,
    };
    direct_token(n, |k| k == op)
}

fn want_space(p: &SyntaxToken, t: &SyntaxToken) -> bool {
    match (p.kind(), t.kind()) {
        (T::Keyword(_), T::Dot) => true,
        (_, T::Comma | T::SemiColon | T::RightPar | T::RightSquare | T::Dot | T::Tick) => false,
        (T::LeftPar | T::LeftSquare | T::Dot | T::Tick | T::CommAt | T::Circ, _) => false,
        (_, T::LeftPar) => match t.parent().kind() {
            N::ParenthesizedName | N::PathnameElement => false,
            N::IndexConstraint => matches!(p.kind(), T::Keyword(_)),
            _ => true,
        },
        (T::Plus | T::Minus, _) => !is_unary_operator(p),
        _ => true,
    }
}

fn is_assignment_operator(parent: N, t: &SyntaxToken) -> bool {
    matches!(t.kind(), T::LTE | T::ColonEq)
        && matches!(
            parent,
            N::SimpleWaveformAssignment
                | N::ConcurrentSimpleSignalAssignment
                | N::SimpleVariableAssignment
                | N::SimpleForceAssignment
        )
}

/// Keywords that start the tail of a declaration, which may fold onto a continuation line.
fn soft_break_before(parent: N, t: &SyntaxToken) -> bool {
    matches!(
        (parent, t.kind()),
        (
            N::AttributeSpecification | N::AliasDeclaration,
            T::Keyword(Kw::Is)
        ) | (N::DisconnectionSpecification, T::Keyword(Kw::After))
            | (
                N::UnboundedArrayDefinition | N::ConstrainedArrayDefinition,
                T::Keyword(Kw::Of)
            )
    )
}

fn is_unary_operator(t: &SyntaxToken) -> bool {
    let parent = t.parent();
    parent.kind() == N::UnaryExpression && parent.first_token().text_offset() == t.text_offset()
}

fn direct_token(n: &SyntaxNode, pred: impl Fn(T) -> bool) -> Option<SyntaxToken> {
    n.children_with_tokens()
        .find_map(|c| c.as_token().filter(|t| pred(t.kind())))
}

fn matching_paren(children: &[SyntaxElement], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in children.iter().enumerate().skip(open) {
        match c.as_token().map(|t| t.kind()) {
            Some(T::LeftPar) => depth += 1,
            Some(T::RightPar) => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn is_separated_list(k: N) -> bool {
    use N::*;
    matches!(
        k,
        AssociationList
            | ElementAssociationList
            | InterfaceList
            | IndexSubtypeDefinitionList
            | EnumerationList
            | SensitivityList
            | EntityClassEntryList
            | ExpressionList
            | RecordResolution
    )
}

/// The items of a parenthesized sequence; each item keeps its trailing separator.
fn list_items(inner: &[SyntaxElement]) -> Vec<Vec<SyntaxElement>> {
    let mut list = match inner {
        [SyntaxElement::Node(n)] => Some(n.clone()),
        _ => None,
    };
    // Look through single-child wrappers such as generic map association wrappers.
    while let Some(n) = list.clone().filter(|n| !is_separated_list(n.kind())) {
        let mut cs = n.children_with_tokens();
        list = match (cs.next(), cs.next()) {
            (Some(SyntaxElement::Node(c)), None) => Some(c),
            _ => None,
        };
    }
    let Some(list) = list else {
        return vec![inner.to_vec()];
    };
    let mut items = vec![Vec::new()];
    for c in list.children_with_tokens() {
        let sep = c
            .as_token()
            .is_some_and(|t| matches!(t.kind(), T::Comma | T::SemiColon));
        items.last_mut().expect("non-empty").push(c);
        if sep {
            items.push(Vec::new());
        }
    }
    items.retain(|i| !i.is_empty());
    items
}

/// A list item without nested parentheses or named associations: short enough to pack.
fn is_simple_item(item: &[SyntaxElement]) -> bool {
    item.iter().all(|e| match e {
        SyntaxElement::Token(t) => matches!(t.kind(), T::Comma),
        SyntaxElement::Node(n) => {
            let mut tokens = Vec::new();
            crate::collect_tokens(n, &mut tokens);
            tokens.iter().all(|t| {
                !matches!(t.kind(), T::LeftPar | T::RightArrow)
                    && !t.leading_trivia().contains_comments()
            })
        }
    })
}

fn first_elements(items: &[Vec<SyntaxElement>]) -> Vec<SyntaxElement> {
    items.iter().filter_map(|i| i.first().cloned()).collect()
}

fn is_named_association(item: &[SyntaxElement]) -> bool {
    item.iter().any(|e| {
        e.as_node()
            .and_then(|n| n.first_child())
            .is_some_and(|c| matches!(c.kind(), N::ElementChoices | N::Formal))
    })
}
