//! Design checks the analyser does not do: the two that open every RTL lint checklist.
//!
//! Both work on the syntax tree, because neither needs a type: a latch is inferred from *where*
//! a signal is assigned, and two drivers are two concurrent statements assigning the same name.
//! They run in the lint layer (`--check lint`) with the rest of the design rules.
//!
//! * `lint_600` — a combinational process assigns a signal on some paths but not all, so the
//!   signal has to remember its value: a latch. The classic causes are an `if` with no `else`
//!   and a `case` alternative that skips a target the others assign.
//! * `lint_601` — a signal is assigned by more than one concurrent statement. Legal for a
//!   resolved type and occasionally deliberate, wrong nearly every other time.

use std::collections::{BTreeMap, BTreeSet};

use crate::Parsed;
use vhdl_syntax::syntax::{NodeKind, SyntaxNode};

use super::lint::Finding;

fn descendants(node: &SyntaxNode, kind: NodeKind, out: &mut Vec<SyntaxNode>) {
    for child in node.children() {
        if child.kind() == kind {
            out.push(child.clone());
        }
        descendants(&child, kind, out);
    }
}

pub fn find(node: &SyntaxNode, kind: NodeKind) -> Vec<SyntaxNode> {
    let mut out = Vec::new();
    descendants(node, kind, &mut out);
    out
}

/// Every token under a node, in order. `SyntaxNode::tokens` only yields the direct ones, and a
/// name is several levels deep.
fn tokens(node: &SyntaxNode, out: &mut Vec<vhdl_syntax::syntax::SyntaxToken>) {
    for element in node.children_with_tokens() {
        match element {
            vhdl_syntax::syntax::SyntaxElement::Node(n) => tokens(&n, out),
            vhdl_syntax::syntax::SyntaxElement::Token(t) => out.push(t),
        }
    }
}

pub fn all_tokens(node: &SyntaxNode) -> Vec<vhdl_syntax::syntax::SyntaxToken> {
    let mut out = Vec::new();
    tokens(node, &mut out);
    out
}

/// The text of a node, lowercased, with the trivia dropped: enough to compare two assignment
/// targets without resolving either.
pub fn text_of(node: &SyntaxNode) -> String {
    all_tokens(node)
        .iter()
        .map(|t| String::from_utf8_lossy(t.text().as_bytes()).to_ascii_lowercase())
        .collect()
}

/// The name a signal assignment writes to, as written.
fn target(assignment: &SyntaxNode) -> Option<(String, usize)> {
    let target = assignment
        .children()
        .find(|c| c.kind() == NodeKind::NameTarget)?;
    let offset = all_tokens(&target).first()?.text_offset();
    // The target as written: `q(3)`, `q(4)`, `rec.a` and `rec` are four different things to
    // drive, and two statements driving different elements of one array is ordinary VHDL.
    let text = text_of(&target);
    (!text.is_empty()).then_some((text, offset))
}

/// Assignments to variables. A variable holds its value between runs of a process, so the
/// question for them is not "assigned on every path" but "read before it was assigned".
const VARIABLE_ASSIGNMENTS: &[NodeKind] = &[
    NodeKind::SimpleVariableAssignment,
    NodeKind::ConditionalVariableAssignment,
    NodeKind::SelectedVariableAssignment,
];

const ASSIGNMENTS: &[NodeKind] = &[
    NodeKind::ConcurrentSimpleSignalAssignment,
    NodeKind::ConcurrentConditionalSignalAssignment,
    NodeKind::ConcurrentSelectedSignalAssignment,
    NodeKind::SimpleWaveformAssignment,
    NodeKind::ConditionalWaveformAssignment,
    NodeKind::SelectedWaveformAssignment,
    NodeKind::SimpleForceAssignment,
    NodeKind::ConditionalForceAssignment,
];

/// Every signal assignment in a node, the node itself included, as (signal, offset).
pub fn assignments(node: &SyntaxNode) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    if ASSIGNMENTS.contains(&node.kind()) {
        out.extend(target(node));
    }
    for kind in ASSIGNMENTS {
        for assignment in find(node, *kind) {
            out.extend(target(&assignment));
        }
    }
    out
}

/// One accepted prefix or suffix. Plain text matches itself; `*` and `?` are wildcards, so
/// `_p?` accepts `_p1` through `_p9`; and `re:` takes a regular expression for anything finer,
/// such as `re:_p[0-9]+`.
pub struct Affix {
    text: String,
    pattern: Option<regex::Regex>,
}

impl Affix {
    pub fn new(text: &str) -> Result<Affix, String> {
        let pattern = match text.strip_prefix("re:") {
            Some(expression) => Some(
                regex::Regex::new(expression)
                    .map_err(|e| format!("`{text}` is not a regular expression: {e}"))?,
            ),
            None => None,
        };
        Ok(Affix {
            text: text.to_owned(),
            pattern,
        })
    }

    fn matches(&self, name: &str, as_prefix: bool) -> bool {
        // Anchored at the start for a prefix, at the end for a suffix.
        if let Some(pattern) = &self.pattern {
            return pattern.find_iter(name).any(|found| {
                if as_prefix {
                    found.start() == 0
                } else {
                    found.end() == name.len()
                }
            });
        }
        let glob = if as_prefix {
            format!("{}*", self.text)
        } else {
            format!("*{}", self.text)
        };
        crate::config::glob(glob.as_bytes(), name.as_bytes())
    }
}

/// How registered signals are expected to be named: `lint_602` for the suffix and `lint_603` for
/// the prefix, each off unless its list is set, so a project can ask for either or both.
pub struct Naming {
    /// `lint_603`.
    pub prefixes: Vec<Affix>,
    /// `lint_602`.
    pub suffixes: Vec<Affix>,
}

impl Naming {
    #[cfg(test)]
    fn none() -> Naming {
        Naming {
            prefixes: Vec::new(),
            suffixes: Vec::new(),
        }
    }

    fn quoted(list: &[Affix]) -> String {
        let items: Vec<String> = list.iter().map(|a| format!("'{}'", a.text)).collect();
        match items.split_last() {
            Some((tail, [])) => tail.clone(),
            Some((tail, head)) => format!("{} or {tail}", head.join(", ")),
            None => String::new(),
        }
    }
}

/// Whether a process is clocked: it tests a clock edge, so what it assigns becomes a register.
pub fn is_clocked(process: &SyntaxNode) -> bool {
    let text = text_of(process);
    text.contains("rising_edge(") || text.contains("falling_edge(") || text.contains("'event")
}

/// Whether a process describes something other than combinational logic, and so cannot infer a
/// latch: a clocked process (an edge test, the shape `vhdl_lang` uses too), or one that suspends
/// on `wait`, which is how testbenches are written and is not synthesisable logic at all.
pub fn is_not_combinational(process: &SyntaxNode) -> bool {
    if !find(process, NodeKind::WaitStatement).is_empty() {
        return true;
    }
    // Combinational logic has a sensitivity list. A process without one runs forever unless it
    // waits, which is how testbench mains are written and is not hardware.
    if find(process, NodeKind::ParenthesizedProcessSensitivityList).is_empty()
        && find(process, NodeKind::AllSensitivityList).is_empty()
    {
        return true;
    }
    let text = text_of(process);
    text.contains("rising_edge(") || text.contains("falling_edge(") || text.contains("'event")
}

/// The signals a sequence of statements assigns on *every* path through it.
///
/// A branch that cannot be shown to assign contributes nothing: an `if` without `else`, a `case`
/// without `others`, a loop that may run zero times. That is what makes the difference between a
/// default assignment (no latch) and a conditional one (latch).
fn always_assigned(statements: &[SyntaxNode], kinds: &[NodeKind]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for statement in statements {
        match statement.kind() {
            NodeKind::IfStatement => {
                let Some(otherwise) = statement
                    .children()
                    .find(|c| c.kind() == NodeKind::IfStatementElse)
                else {
                    continue; // No `else`: some path assigns nothing.
                };
                let mut branches: Vec<BTreeSet<String>> =
                    vec![always_assigned(&body(statement), kinds)];
                branches.extend(
                    statement
                        .children()
                        .filter(|c| c.kind() == NodeKind::IfStatementElsif)
                        .map(|elsif| always_assigned(&body(&elsif), kinds)),
                );
                branches.push(always_assigned(&body(&otherwise), kinds));
                out.extend(intersection(branches));
            }
            NodeKind::CaseStatement => {
                let alternatives: Vec<SyntaxNode> = statement
                    .children()
                    .filter(|c| c.kind() == NodeKind::CaseStatementAlternative)
                    .collect();
                // A case must cover its type, but only `others` says so without knowing the type.
                let covers_everything = alternatives
                    .iter()
                    .any(|a| text_of(a).starts_with("whenothers"));
                if !covers_everything || alternatives.is_empty() {
                    continue;
                }
                out.extend(intersection(
                    alternatives
                        .iter()
                        .map(|a| always_assigned(&body(a), kinds))
                        .collect(),
                ));
            }
            // A `for` loop over a range runs, and synthesis unrolls it, so its body assigns.
            // A `while` loop may not run at all, so it promises nothing.
            NodeKind::LoopStatement
                if statement
                    .children()
                    .find(|c| c.kind() == NodeKind::LoopStatementPreamble)
                    .is_some_and(|p| text_of(&p).starts_with("for")) =>
            {
                out.extend(always_assigned(&body(statement), kinds));
            }
            kind if kinds.contains(&kind) => {
                out.extend(target(statement).map(|(name, _)| name));
            }
            _ => {}
        }
    }
    out
}

fn intersection(sets: Vec<BTreeSet<String>>) -> BTreeSet<String> {
    let mut iter = sets.into_iter();
    let first = iter.next().unwrap_or_default();
    iter.fold(first, |acc, set| &acc & &set)
}

/// The statements a node holds: those of its `SequenceOfStatements`, or its own children when it
/// keeps them directly, as a process does.
fn body(node: &SyntaxNode) -> Vec<SyntaxNode> {
    let sequences: Vec<SyntaxNode> = node
        .children()
        .filter(|c| c.kind() == NodeKind::SequenceOfStatements)
        .collect();
    if sequences.is_empty() {
        return node.children().collect();
    }
    sequences.iter().flat_map(SyntaxNode::children).collect()
}

/// The variables a process declares.
fn declared_variables(process: &SyntaxNode) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for declaration in find(process, NodeKind::VariableDeclaration) {
        // `variable v : t := x;` has a value from elaboration, so reading it is not a latch.
        if text_of(&declaration).contains(":=") {
            continue;
        }
        for token in all_tokens(&declaration) {
            let text = String::from_utf8_lossy(token.text().as_bytes()).to_ascii_lowercase();
            if text == ":" {
                break; // The type follows; only the names come before it.
            }
            if text
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && text != "variable"
                && text != "shared"
            {
                out.insert(text);
            }
        }
    }
    out
}

/// The names a statement reads, which is everything it names except the targets it assigns.
pub fn reads(node: &SyntaxNode, out: &mut Vec<(String, usize)>) {
    let assignment =
        ASSIGNMENTS.contains(&node.kind()) || VARIABLE_ASSIGNMENTS.contains(&node.kind());
    for child in node.children() {
        // The left-hand side of an assignment is written, not read; anything else, including the
        // index expressions inside `q(i)`, is read.
        if assignment && child.kind() == NodeKind::NameTarget {
            for grandchild in child.children() {
                if grandchild.kind() != NodeKind::Name {
                    reads(&grandchild, out);
                }
            }
            continue;
        }
        reads(&child, out);
    }
    if matches!(node.kind(), NodeKind::Name | NodeKind::NameDesignatorPrefix)
        && let Some(token) = all_tokens(node).first()
    {
        let text = String::from_utf8_lossy(token.text().as_bytes()).to_ascii_lowercase();
        out.push((text, token.text_offset()));
    }
}

/// Variables read on a path that has not assigned them yet: the value has to survive from the
/// previous run of the process, which is a latch just as much as an unassigned signal is.
fn variable_latches(process: &SyntaxNode, statements: &[SyntaxNode]) -> Vec<(String, usize)> {
    let declared = declared_variables(process);
    if declared.is_empty() {
        return Vec::new();
    }
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    let mut assigned: BTreeSet<String> = BTreeSet::new();
    for statement in statements {
        let mut named = Vec::new();
        reads(statement, &mut named);
        for (name, offset) in named {
            if declared.contains(&name) && !assigned.contains(&name) {
                out.entry(name).or_insert(offset);
            }
        }
        // By the object, not the element: a loop that fills `v(0)` and then reads `v(0)` on the
        // next iteration is ordinary code, and proving that needs more than syntax.
        assigned.extend(
            always_assigned(std::slice::from_ref(statement), VARIABLE_ASSIGNMENTS)
                .iter()
                .map(|name| path(name).0),
        );
    }
    out.into_iter().collect()
}

/// Signals a process assigns somewhere but not on every path: each has to remember its value.
fn latches(process: &SyntaxNode) -> Vec<(String, usize)> {
    let statements: Vec<SyntaxNode> = find(process, NodeKind::ProcessStatementPart)
        .iter()
        .flat_map(body)
        .collect();
    let always = always_assigned(&statements, ASSIGNMENTS);
    // Assigning a whole object assigns its parts, so `rec` covers `rec.a` and `q` covers `q(3)`.
    // (For counting drivers the parts stay apart; here the question is only whether a value was
    // given at all.)
    let covered = |name: &str| {
        always.contains(name)
            || name
                .match_indices(['.', '('])
                .any(|(at, _)| always.contains(&name[..at]))
    };
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    for (name, offset) in assignments(process) {
        if !covered(&name) {
            out.entry(name).or_insert(offset);
        }
    }
    for (name, offset) in variable_latches(process, &statements) {
        out.entry(name).or_insert(offset);
    }
    out.into_iter().collect()
}

/// One step of an assignment target: `.field`, or `(index)` with one entry per dimension.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Selector {
    Field(String),
    Index(Vec<String>),
}

/// Split `rec.arr(3)(1, 2).f` into its base name and the steps that follow it.
pub fn path(target: &str) -> (String, Vec<Selector>) {
    let mut base = String::new();
    let mut selectors: Vec<Selector> = Vec::new();
    let mut rest = target;
    loop {
        let stop = rest.find(['.', '(']).unwrap_or(rest.len());
        let (word, tail) = rest.split_at(stop);
        if base.is_empty() && selectors.is_empty() {
            base.push_str(word);
        } else if !word.is_empty() {
            selectors.push(Selector::Field(word.to_owned()));
        }
        match tail.chars().next() {
            Some('.') => rest = &tail[1..],
            Some('(') => {
                // Find the parenthesis this one closes, so nested parentheses travel with it.
                let mut depth = 0usize;
                let mut end = tail.len();
                for (at, c) in tail.char_indices() {
                    match c {
                        '(' => depth += 1,
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                end = at;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                let inside = tail.get(1..end).unwrap_or("");
                selectors.push(Selector::Index(
                    inside.split(',').map(|d| d.trim().to_owned()).collect(),
                ));
                rest = tail.get(end + 1..).unwrap_or("");
            }
            _ => break,
        }
        if rest.is_empty() {
            break;
        }
    }
    (base, selectors)
}

/// `3` as `(3, 3)`, `7 downto 4` as `(4, 7)`; `None` when it is not literal.
fn static_range(text: &str) -> Option<(i64, i64)> {
    let number = |t: &str| t.trim().parse::<i64>().ok();
    if let Some(n) = number(text) {
        return Some((n, n));
    }
    let lower = text.to_ascii_lowercase();
    for keyword in [" downto ", " to "] {
        if let Some((left, right)) = lower.split_once(keyword) {
            let (a, b) = (number(left)?, number(right)?);
            return Some((a.min(b), a.max(b)));
        }
    }
    None
}

/// Whether two index selectors can name the same element. Unknown means "not proven to overlap"
/// and nothing is reported then: `q(i)` and `q(j)` are usually two branches of a generate, and a
/// false accusation of a double driver is worse than a missed one.
fn indices_overlap(left: &[String], right: &[String]) -> bool {
    if left == right {
        return true;
    }
    if left.len() != right.len() {
        return false;
    }
    // Every dimension has to overlap for the elements to meet.
    left.iter().zip(right).all(|(a, b)| {
        if a == b {
            return true;
        }
        match (static_range(a), static_range(b)) {
            (Some((a0, a1)), Some((b0, b1))) => a0 <= b1 && b0 <= a1,
            _ => false,
        }
    })
}

/// Whether two assignment targets can drive the same bit.
fn targets_overlap(left: &str, right: &str) -> bool {
    let (left_base, left_path) = path(left);
    let (right_base, right_path) = path(right);
    if left_base != right_base {
        return false;
    }
    for (a, b) in left_path.iter().zip(&right_path) {
        match (a, b) {
            (Selector::Field(x), Selector::Field(y)) if x != y => return false,
            (Selector::Index(x), Selector::Index(y)) if !indices_overlap(x, y) => return false,
            (Selector::Field(_), Selector::Index(_)) | (Selector::Index(_), Selector::Field(_)) => {
                return false;
            }
            _ => {}
        }
    }
    // Either the same target, or one is the whole of what the other is a part of.
    true
}

pub fn check(parsed: &Parsed, file: &std::path::Path, naming: &Naming) -> Vec<Finding> {
    let mut findings = Vec::new();
    let at = |offset: usize| parsed.line_col(offset);

    for architecture in find(parsed.root(), NodeKind::ArchitectureBody) {
        // lint_601: two concurrent statements that can drive the same bit. Targets are compared
        // as designator paths, so `q(3)` and `q(4)`, `rec.a` and `rec.b`, and `a(0)(1)` and
        // `a(0)(2)` are different things to drive, while `q` and `q(3)` are not.
        // `scope` separates drivers that cannot both exist: two branches of an `if ... generate`,
        // or two separate generates with complementary conditions, elaborate one at a time.
        // Proving which needs the generic's value, so drivers in different generate bodies are
        // simply never compared -- a missed conflict rather than an invented one.
        let mut drivers: Vec<(String, usize, usize)> = Vec::new();
        let mut scopes: Vec<(SyntaxNode, usize)> =
            find(&architecture, NodeKind::ArchitectureStatementPart)
                .iter()
                .flat_map(SyntaxNode::children)
                .map(|statement| (statement, 0usize))
                .collect();
        while let Some((statement, scope)) = scopes.pop() {
            if matches!(
                statement.kind(),
                NodeKind::IfGenerateStatement
                    | NodeKind::CaseGenerateStatement
                    | NodeKind::ForGenerateStatement
            ) {
                // The bodies of this generate only: a nested one is reached when its own
                // statement is popped, and finding it twice would double every driver in it.
                let bodies = statement.children().flat_map(|branch| {
                    if branch.kind() == NodeKind::GenerateStatementBody {
                        vec![branch]
                    } else {
                        branch
                            .children()
                            .filter(|c| c.kind() == NodeKind::GenerateStatementBody)
                            .collect()
                    }
                });
                for body in bodies {
                    let key = all_tokens(&body)
                        .first()
                        .map_or(0, vhdl_syntax::syntax::SyntaxToken::text_offset);
                    scopes.extend(body.children().map(|child| (child, key)));
                }
                continue;
            }
            // Within one statement a signal is driven once, however often and through whatever
            // parts it is assigned: a process writing `q` and later `q(3)` is still one driver.
            let mut seen: Vec<String> = Vec::new();
            for (name, offset) in assignments(&statement) {
                if seen.iter().any(|other| targets_overlap(&name, other)) {
                    continue;
                }
                seen.push(name.clone());
                drivers.push((name, offset, scope));
            }
        }
        for (i, (name, offset, scope)) in drivers.iter().enumerate() {
            let mut offsets: Vec<usize> = drivers
                .iter()
                .enumerate()
                .filter(|(j, (other, _, other_scope))| {
                    *j != i && other_scope == scope && targets_overlap(name, other)
                })
                .map(|(_, (_, other, _))| *other)
                .collect();
            if offsets.is_empty() {
                continue;
            }
            offsets.push(*offset);
            offsets.sort_unstable();
            offsets.dedup();
            // One conflict, one finding: only the earliest driver of a group reports.
            if offsets.len() < 2 || offsets.first() != Some(offset) {
                continue;
            }
            let (line, column) = at(*offset);
            // Each driver is a place of its own, not a line number inside a sentence.
            let related = offsets
                .iter()
                .map(|o| {
                    let (line, column) = at(*o);
                    super::lint::Related {
                        file: file.to_path_buf(),
                        line,
                        column,
                        message: format!("'{name}' is driven here"),
                    }
                })
                .collect();
            findings.push(Finding {
                file: file.to_path_buf(),
                rule: "lint_601",
                line,
                column,
                message: format!(
                    "Signal '{name}' is assigned by {} concurrent statements",
                    offsets.len()
                ),
                related,
            });
        }

        // lint_602: a signal a clocked process assigns becomes a register, and a project may
        // want to see that in its name.
        for process in find(&architecture, NodeKind::ProcessStatement) {
            if !is_clocked(&process) {
                continue;
            }
            let mut seen: BTreeSet<String> = BTreeSet::new();
            for (target, offset) in assignments(&process) {
                let name = path(&target).0;
                if name.is_empty() || !seen.insert(name.clone()) {
                    continue;
                }
                let (line, column) = at(offset);
                let mut complain = |rule, what: &str, accepted: &[Affix]| {
                    findings.push(Finding {
                        file: file.to_path_buf(),
                        rule,
                        line,
                        column,
                        message: format!(
                            "Registered signal '{name}' does not have the {what} {}",
                            Naming::quoted(accepted)
                        ),
                        related: Vec::new(),
                    });
                };
                if !naming.suffixes.is_empty()
                    && !naming.suffixes.iter().any(|s| s.matches(&name, false))
                {
                    complain("lint_602", "suffix", &naming.suffixes);
                }
                if !naming.prefixes.is_empty()
                    && !naming.prefixes.iter().any(|p| p.matches(&name, true))
                {
                    complain("lint_603", "prefix", &naming.prefixes);
                }
            }
        }

        // lint_600: a combinational process that does not assign on every path.
        for process in find(&architecture, NodeKind::ProcessStatement) {
            if is_not_combinational(&process) {
                continue;
            }
            for (name, offset) in latches(&process) {
                let (line, column) = at(offset);
                findings.push(Finding {
                    file: file.to_path_buf(),
                    rule: "lint_600",
                    line,
                    column,
                    message: format!(
                        "Signal '{name}' is not assigned on every path of this combinational \
                         process, which infers a latch"
                    ),
                    related: Vec::new(),
                });
            }
        }
    }
    findings.sort_by_key(|f| (f.line, f.column, f.rule));
    findings
}

/// The rules this module reports, for `--list_rules`.
pub const RULES: &[(&str, &str)] = &[
    (
        "lint_600",
        "A combinational process does not assign a signal on every path, inferring a latch.",
    ),
    (
        "lint_601",
        "A signal is assigned by more than one concurrent statement.",
    ),
    (
        "lint_602",
        "A signal assigned by a clocked process does not have a register suffix (off by \
         default; set `suffixes`).",
    ),
    (
        "lint_603",
        "A signal assigned by a clocked process does not have a register prefix (off by \
         default; set `prefixes`).",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn check_source(text: &str) -> Vec<(&'static str, usize)> {
        let parsed = Parsed::new(text.as_bytes().to_vec());
        assert!(parsed.syntax_errors().is_empty(), "test source must parse");
        check(
            &parsed,
            std::path::Path::new("dut.vhd"),
            &Naming {
                prefixes: Vec::new(),
                suffixes: Vec::new(),
            },
        )
        .into_iter()
        .map(|f| (f.rule, f.line))
        .collect()
    }

    const PREAMBLE: &str = "entity dut is\nend entity;\n\narchitecture rtl of dut is\n  \
                            signal a, b, c, d, clk : bit;\nbegin\n";

    #[test]
    fn every_driver_is_a_related_location() {
        let source = "entity dut is\nend entity dut;\n\narchitecture rtl of dut is\n  \
                      signal a, b, q : bit;\nbegin\n  q <= a;\n  q <= b;\n\
                      end architecture rtl;\n";
        let parsed = Parsed::new(source.as_bytes().to_vec());
        let found = check(
            &parsed,
            std::path::Path::new("dut.vhd"),
            &Naming {
                prefixes: Vec::new(),
                suffixes: Vec::new(),
            },
        );
        let drivers: Vec<&Finding> = found.iter().filter(|f| f.rule == "lint_601").collect();
        assert_eq!(drivers.len(), 1, "{found:?}");
        // The lines belong to the finding, not to its message.
        assert_eq!(drivers[0].related.len(), 2, "{:?}", drivers[0].related);
        assert!(
            !drivers[0].message.contains("lines"),
            "locations should not be written into the message: {}",
            drivers[0].message
        );
    }

    #[test]
    fn an_if_without_else_infers_a_latch() {
        let found = check_source(&format!(
            "{PREAMBLE}  p : process (a, b) is\n  begin\n    if a = '1' then\n      \
             c <= b;\n    end if;\n  end process;\nend architecture;\n"
        ));
        assert!(
            found.iter().any(|(rule, _)| *rule == "lint_600"),
            "got {found:?}"
        );
    }

    #[test]
    fn a_default_assignment_prevents_the_latch() {
        let found = check_source(&format!(
            "{PREAMBLE}  p : process (a, b) is\n  begin\n    c <= '0';\n    if a = '1' then\n      \
             c <= b;\n    end if;\n  end process;\nend architecture;\n"
        ));
        assert!(
            !found.iter().any(|(rule, _)| *rule == "lint_600"),
            "an unconditional assignment is a default, got {found:?}"
        );
    }

    #[test]
    fn a_process_that_waits_is_not_a_latch() {
        let found = check_source(&format!(
            "{PREAMBLE}  p : process is\n  begin\n    wait until a = '1';\n    if b = '1' then\n      \
             c <= b;\n    end if;\n    wait;\n  end process;\nend architecture;\n"
        ));
        assert!(
            !found.iter().any(|(rule, _)| *rule == "lint_600"),
            "a testbench process is not combinational logic, got {found:?}"
        );
    }

    #[test]
    fn a_default_inside_a_branch_prevents_the_latch() {
        let found = check_source(&format!(
            "{PREAMBLE}  p : process (a, b) is\n  begin\n    if a = '1' then\n      \
             d <= '0';\n      if b = '1' then\n        d <= '1';\n      end if;\n    else\n      \
             d <= '0';\n    end if;\n  end process;\nend architecture;\n"
        ));
        assert!(
            !found.iter().any(|(rule, _)| *rule == "lint_600"),
            "every path assigns d, got {found:?}"
        );
    }

    #[test]
    fn a_nested_if_without_else_is_a_latch() {
        let found = check_source(&format!(
            "{PREAMBLE}  p : process (a, b) is\n  begin\n    if a = '1' then\n      \
             c <= '1';\n      if b = '1' then\n        d <= '1';\n      end if;\n    else\n      \
             c <= '0';\n      d <= '0';\n    end if;\n  end process;\nend architecture;\n"
        ));
        assert!(
            found.iter().any(|(rule, _)| *rule == "lint_600"),
            "a = '1' with b = '0' leaves d unassigned, got {found:?}"
        );
    }

    #[test]
    fn a_case_with_others_assigning_everywhere_is_not_a_latch() {
        let found = check_source(&format!(
            "{PREAMBLE}  p : process (a) is\n  begin\n    case a is\n      when '0' =>\n        \
             c <= '0';\n      when others =>\n        c <= '1';\n    end case;\n  \
             end process;\nend architecture;\n"
        ));
        assert!(
            !found.iter().any(|(rule, _)| *rule == "lint_600"),
            "every alternative assigns c, got {found:?}"
        );
    }

    #[test]
    fn assigning_a_whole_record_covers_its_fields() {
        let found = check_source(
            "entity dut is\nend entity;\n\narchitecture rtl of dut is\n  \
             type rec_t is record\n    x : bit;\n    y : bit;\n  end record;\n  \
             constant init : rec_t := (others => '0');\n  signal a : bit;\n  \
             signal r : rec_t;\nbegin\n  p : process (a) is\n  begin\n    \
             if a = '1' then\n      r <= init;\n    else\n      r <= init;\n      \
             r.x <= a;\n    end if;\n  end process;\nend architecture;\n",
        );
        assert!(
            !found.iter().any(|(rule, _)| *rule == "lint_600"),
            "every path assigns the whole record, got {found:?}"
        );
    }

    #[test]
    fn a_variable_read_before_it_is_written_is_a_latch() {
        let found = check_source(&format!(
            "{PREAMBLE}  p : process (a, b) is\n    variable v : bit;\n  begin\n    \
             if a = '1' then\n      v := b;\n    end if;\n    c <= v;\n  \
             end process;\nend architecture;\n"
        ));
        assert!(
            found.iter().any(|(rule, _)| *rule == "lint_600"),
            "v survives from the previous run, got {found:?}"
        );
    }

    #[test]
    fn a_variable_written_before_it_is_read_is_fine() {
        let found = check_source(&format!(
            "{PREAMBLE}  p : process (a, b) is\n    variable v : bit;\n  begin\n    \
             v := '0';\n    if a = '1' then\n      v := b;\n    end if;\n    c <= v;\n  \
             end process;\nend architecture;\n"
        ));
        assert!(
            !found.iter().any(|(rule, _)| *rule == "lint_600"),
            "v has a default before it is read, got {found:?}"
        );
    }

    #[test]
    fn a_variable_in_a_clocked_process_is_not_a_latch() {
        let found = check_source(&format!(
            "{PREAMBLE}  p : process (clk) is\n    variable v : bit;\n  begin\n    \
             if rising_edge(clk) then\n      c <= v;\n      v := a;\n    end if;\n  \
             end process;\nend architecture;\n"
        ));
        assert!(
            !found.iter().any(|(rule, _)| *rule == "lint_600"),
            "a variable in a register is memory on purpose, got {found:?}"
        );
    }

    #[test]
    fn a_for_loop_assigns_every_element() {
        let found = check_source(
            "entity dut is\nend entity;\n\narchitecture rtl of dut is\n  \
             signal a, b : bit_vector(3 downto 0);\nbegin\n  p : process (a) is\n  begin\n    \
             for i in a'range loop\n      b(i) <= a(i);\n    end loop;\n  end process;\n\
             end architecture;\n",
        );
        assert!(
            !found.iter().any(|(rule, _)| *rule == "lint_600"),
            "the loop covers every bit, got {found:?}"
        );
    }

    #[test]
    fn a_clocked_process_is_not_a_latch() {
        let found = check_source(&format!(
            "{PREAMBLE}  p : process (clk) is\n  begin\n    if rising_edge(clk) then\n      \
             c <= b;\n    end if;\n  end process;\nend architecture;\n"
        ));
        assert!(
            !found.iter().any(|(rule, _)| *rule == "lint_600"),
            "a register is not a latch, got {found:?}"
        );
    }

    #[test]
    fn two_processes_driving_one_signal() {
        let found = check_source(&format!(
            "{PREAMBLE}  p : process (a) is\n  begin\n    c <= a;\n  end process;\n\n  \
             q : process (b) is\n  begin\n    c <= b;\n  end process;\nend architecture;\n"
        ));
        assert!(
            found.iter().any(|(rule, _)| *rule == "lint_601"),
            "got {found:?}"
        );
    }

    #[test]
    fn a_concurrent_assignment_counts_as_a_driver() {
        let found = check_source(&format!(
            "{PREAMBLE}  c <= a;\n\n  p : process (b) is\n  begin\n    c <= b;\n  \
             end process;\nend architecture;\n"
        ));
        assert!(
            found.iter().any(|(rule, _)| *rule == "lint_601"),
            "got {found:?}"
        );
    }

    fn naming(prefixes: &[&str], suffixes: &[&str]) -> Naming {
        let affixes = |list: &[&str]| list.iter().map(|a| Affix::new(a).expect("affix")).collect();
        Naming {
            prefixes: affixes(prefixes),
            suffixes: affixes(suffixes),
        }
    }

    fn check_named(source: &str, naming: &Naming) -> Vec<(&'static str, String)> {
        let parsed = Parsed::new(source.as_bytes().to_vec());
        assert!(parsed.syntax_errors().is_empty(), "test source must parse");
        check(&parsed, std::path::Path::new("dut.vhd"), naming)
            .into_iter()
            .map(|f| (f.rule, f.message))
            .collect()
    }

    const CLOCKED: &str = "entity dut is\nend entity;\n\narchitecture rtl of dut is\n  \
                           signal clk, d, state, count_q : bit;\nbegin\n  \
                           p : process (clk) is\n  begin\n    if rising_edge(clk) then\n      \
                           state <= d;\n      count_q <= d;\n    end if;\n  end process;\n\
                           end architecture;\n";

    #[test]
    fn registered_signals_are_only_checked_when_asked() {
        let found = check_named(CLOCKED, &Naming::none());
        assert!(
            !found.iter().any(|(rule, _)| *rule == "lint_602"),
            "nothing configured, nothing to say: {found:?}"
        );
    }

    #[test]
    fn a_register_without_the_suffix_is_reported() {
        let found = check_named(CLOCKED, &naming(&[], &["_q", "_r"]));
        let reported: Vec<&String> = found
            .iter()
            .filter(|(rule, _)| *rule == "lint_602")
            .map(|(_, message)| message)
            .collect();
        assert_eq!(reported.len(), 1, "{found:?}");
        assert!(reported[0].contains("'state'"), "{reported:?}");
        assert!(reported[0].contains("'_q' or '_r'"), "{reported:?}");
    }

    #[test]
    fn a_prefix_works_as_well_as_a_suffix() {
        let found = check_named(CLOCKED, &naming(&["r_"], &[]));
        assert_eq!(
            found.iter().filter(|(rule, _)| *rule == "lint_603").count(),
            2,
            "neither name has the prefix: {found:?}"
        );
    }

    #[test]
    fn a_combinational_signal_is_not_a_register() {
        let found = check_named(
            &format!(
                "{PREAMBLE}  p : process (a) is\n  begin\n    c <= a;\n  \
                      end process;\nend architecture;\n"
            ),
            &naming(&[], &["_q"]),
        );
        assert!(
            !found.iter().any(|(rule, _)| *rule == "lint_602"),
            "no clock, no register: {found:?}"
        );
    }

    #[test]
    fn a_suffix_can_be_a_pattern() {
        // Pipeline stages: `_p1`, `_p2`, ... without listing each one.
        let source = "entity dut is\nend entity;\n\narchitecture rtl of dut is\n  \
                      signal clk, d, a_p1, b_p12, c_comb : bit;\nbegin\n  \
                      p : process (clk) is\n  begin\n    if rising_edge(clk) then\n      \
                      a_p1 <= d;\n      b_p12 <= d;\n      c_comb <= d;\n    end if;\n  \
                      end process;\nend architecture;\n";
        for pattern in ["_p*", "re:_p[0-9]+"] {
            let found = check_named(source, &naming(&[], &[pattern]));
            let reported: Vec<&String> = found
                .iter()
                .filter(|(rule, _)| *rule == "lint_602")
                .map(|(_, message)| message)
                .collect();
            assert_eq!(reported.len(), 1, "{pattern}: {found:?}");
            assert!(reported[0].contains("'c_comb'"), "{pattern}: {reported:?}");
        }
        // A single wildcard character accepts one digit only.
        let found = check_named(source, &naming(&[], &["_p?"]));
        assert_eq!(
            found.iter().filter(|(rule, _)| *rule == "lint_602").count(),
            2,
            "`_p?` accepts _p1 but not _p12: {found:?}"
        );
    }

    #[test]
    fn overlapping_targets() {
        // The same thing, twice.
        assert!(targets_overlap("q", "q"));
        assert!(targets_overlap("q(3)", "q(3)"));
        // A whole object and a part of it.
        assert!(targets_overlap("q", "q(3)"));
        assert!(targets_overlap("rec", "rec.a"));
        assert!(targets_overlap("rec", "rec.a.b(2)"));
        // Different elements, fields and dimensions are different drivers.
        assert!(!targets_overlap("q(3)", "q(4)"));
        assert!(!targets_overlap("rec.a", "rec.b"));
        assert!(!targets_overlap("rec.a.x", "rec.a.y"));
        assert!(!targets_overlap("arr(0).f", "arr(1).f"));
        assert!(!targets_overlap("a(0)(1)", "a(0)(2)"));
        assert!(!targets_overlap("a(1, 2)", "a(1, 3)"));
        assert!(!targets_overlap("other", "q"));
        // Slices meet or they do not.
        assert!(!targets_overlap("q(3 downto 0)", "q(7 downto 4)"));
        assert!(targets_overlap("q(7 downto 0)", "q(3)"));
        assert!(targets_overlap("q(7 downto 4)", "q(5 downto 0)"));
        // The same field of the same element is one driver.
        assert!(targets_overlap("arr(0).f", "arr(0).f"));
        // What cannot be evaluated is not accused: two generate branches are the usual case.
        assert!(!targets_overlap("q(i)", "q(j)"));
        assert!(targets_overlap("q(i)", "q(i)"));
    }

    #[test]
    fn different_elements_are_not_two_drivers() {
        let found = check_source(
            "entity dut is\nend entity;\n\narchitecture rtl of dut is\n  \
             signal a : bit;\n  signal q : bit_vector(7 downto 0);\nbegin\n  \
             q(3 downto 0) <= (others => a);\n  q(7 downto 4) <= (others => '0');\n\
             end architecture;\n",
        );
        assert!(
            !found.iter().any(|(rule, _)| *rule == "lint_601"),
            "disjoint slices, got {found:?}"
        );
    }

    #[test]
    fn a_whole_object_and_a_part_of_it_are_two_drivers() {
        let found = check_source(
            "entity dut is\nend entity;\n\narchitecture rtl of dut is\n  \
             signal a : bit;\n  signal q : bit_vector(7 downto 0);\nbegin\n  \
             q <= (others => '0');\n  q(3) <= a;\nend architecture;\n",
        );
        assert!(
            found.iter().any(|(rule, _)| *rule == "lint_601"),
            "the whole vector and one of its bits, got {found:?}"
        );
    }

    #[test]
    fn one_driver_is_not_reported() {
        let found = check_source(&format!(
            "{PREAMBLE}  c <= a;\n  d <= b;\nend architecture;\n"
        ));
        assert!(
            !found.iter().any(|(rule, _)| *rule == "lint_601"),
            "got {found:?}"
        );
    }
}
