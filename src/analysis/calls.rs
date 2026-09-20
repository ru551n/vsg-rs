//! Which subprogram calls which, and the cycles in that.
//!
//! Recursion is legal VHDL and no simulator objects to it. No synthesis tool accepts it: the
//! hardware for a subprogram is inlined at each call, and a call that reaches itself has no
//! bottom to inline from. A recursive function in a package that RTL also uses is therefore a
//! synthesis failure waiting for whoever instantiates it, reported here rather than by a vendor
//! tool much later.
//!
//! Only *direct* recursion is reported: a subprogram whose body calls itself. VHDL requires a
//! forward declaration for two subprograms to call each other, and a call to a name that has one
//! resolves to the declaration rather than to the body, so a mutual cycle is not visible here.
//! Under-reporting is the right way to be wrong about this.
//!
//! **This cannot be done from the syntax.** A function whose body names itself is nearly always
//! calling a *different* subprogram of the same name: scanning `VUnit` for the shape finds 1871
//! candidates, almost none of them recursive, because VHDL overloads heavily -- `to_slv` calling
//! another `to_slv` is the normal case. Only the resolved symbol table tells the two apart, so
//! this module works from `vhdl_lang`'s analysed project and nothing else.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use vhdl_lang::ast::search::{DeclarationItem, FoundDeclaration, SearchState, Searcher};
use vhdl_lang::{EntityId, Project, Reference, SrcPos, TokenAccess};

use super::lint::Finding;

/// One subprogram body, and the span its calls live in.
struct Body {
    name: String,
    /// Where the subprogram is declared, for the finding.
    at: SrcPos,
    /// Begin to end, so a call can be attributed to the subprogram it is written in.
    span: SrcPos,
}

/// Collects subprogram bodies and every resolved reference, in one walk of the design.
#[derive(Default)]
struct Calls {
    bodies: HashMap<EntityId, Body>,
    references: Vec<(SrcPos, EntityId)>,
}

impl Searcher for Calls {
    fn search_decl(&mut self, ctx: &dyn TokenAccess, decl: FoundDeclaration<'_>) -> SearchState {
        if let DeclarationItem::Subprogram(body) = decl.ast
            && let Some(id) = decl.reference.get()
        {
            let designator = body.specification.subpgm_designator();
            self.bodies.insert(
                id,
                Body {
                    name: format!("{}", designator.item),
                    at: ctx.get_pos(designator.token).clone(),
                    span: ctx.get_span(body.begin_token, body.end_token),
                },
            );
        }
        SearchState::NotFinished
    }

    fn search_pos_with_ref(
        &mut self,
        _ctx: &dyn TokenAccess,
        pos: &SrcPos,
        reference: &Reference,
    ) -> SearchState {
        if let Some(id) = reference.get() {
            self.references.push((pos.clone(), id));
        }
        SearchState::NotFinished
    }
}

/// Whether `inner` falls inside `outer`, in the same file.
fn contains(outer: &SrcPos, inner: &SrcPos) -> bool {
    outer.source == inner.source
        && outer.range.start <= inner.range.start
        && inner.range.end <= outer.range.end
}

/// The subprograms that call themselves.
///
/// `wanted` maps a canonical path to the name the run was given, so a recursive subprogram in
/// one of the standard libraries is not reported against a project that merely uses it.
pub(crate) fn recursion(project: &Project, wanted: &BTreeMap<PathBuf, PathBuf>) -> Vec<Finding> {
    let mut calls = Calls::default();
    project.search(&mut calls);

    let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let mut recursive: HashSet<EntityId> = HashSet::new();
    for (pos, callee) in &calls.references {
        // A call is this subprogram's own only if it is written inside that subprogram's body.
        // The innermost body wins, so a nested subprogram calling its parent's name is not read
        // as the parent calling itself.
        let caller = calls
            .bodies
            .iter()
            .filter(|(_, body)| contains(&body.span, pos))
            .min_by_key(|(_, body)| body.span.range.end.line - body.span.range.start.line)
            .map(|(id, _)| *id);
        if caller == Some(*callee) {
            recursive.insert(*callee);
        }
    }

    let mut findings: Vec<Finding> = recursive
        .iter()
        .filter_map(|id| {
            let body = calls.bodies.get(id)?;
            let given = wanted.get(&canonical(body.at.source.file_name()))?;
            Some(Finding {
                file: given.clone(),
                rule: "lint_760",
                line: body.at.range.start.line as usize + 1,
                column: body.at.range.start.character as usize + 1,
                message: format!(
                    "Subprogram '{}' calls itself. Recursion is legal VHDL and simulates, but no \
                     synthesis tool accepts it: the hardware for a call is inlined, and a call \
                     that reaches itself has no bottom to inline from.",
                    body.name
                ),
                related: Vec::new(),
            })
        })
        .collect();
    findings.sort_by(|a, b| (&a.file, a.line, a.column).cmp(&(&b.file, b.line, b.column)));
    findings
}

/// The rules this module reports, for `--list_rules`.
pub const RULES: &[super::Rule] = &[super::Rule {
    id: "lint_760",
    description: "A subprogram whose body calls itself, which no synthesis tool accepts (off by default).",
    certainty: super::Certainty::Advisory,
}];

#[cfg(test)]
mod tests {
    use super::super::lint::{Analyser, Source};

    /// The names `lint_760` reports for one package.
    fn recursive(source: &str) -> Vec<String> {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join("p.vhd");
        std::fs::write(&path, source).expect("write");
        let sources = [Source::file(path)];
        let mut analyser = Analyser::new(&sources).expect("analyser");
        let _ = analyser.analyse(&sources);
        analyser
            .recursion(&sources)
            .into_iter()
            .map(|f| f.message)
            .collect()
    }

    #[test]
    fn a_function_that_calls_itself() {
        let found = recursive(
            "package p is\n  function fact (n : integer) return integer;\nend package p;\n\n\
             package body p is\n  function fact (n : integer) return integer is\n  begin\n    \
             if n <= 1 then\n      return 1;\n    end if;\n    return n * fact(n - 1);\n  \
             end function fact;\nend package body p;\n",
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("'fact' calls itself"), "{found:?}");
    }

    #[test]
    fn an_overload_of_the_same_name_is_not_recursion() {
        // `to_slv` calling a different `to_slv` is the normal VHDL idiom, and is the reason this
        // rule cannot be written against the syntax tree.
        let found = recursive(
            "package p is\n  function to_slv (v : bit) return bit_vector;\n  \
             function to_slv (v : integer) return bit_vector;\nend package p;\n\n\
             package body p is\n  function to_slv (v : bit) return bit_vector is\n  begin\n    \
             return \"\" & v;\n  end function to_slv;\n\n  \
             function to_slv (v : integer) return bit_vector is\n    variable b : bit := '1';\n  \
             begin\n    return to_slv(b);\n  end function to_slv;\nend package body p;\n",
        );
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_subprogram_that_calls_another_is_not_recursion() {
        let found = recursive(
            "package p is\n  function one return integer;\n  function two return integer;\n\
             end package p;\n\npackage body p is\n  function one return integer is\n  begin\n    \
             return 1;\n  end function one;\n\n  function two return integer is\n  begin\n    \
             return one + 1;\n  end function two;\nend package body p;\n",
        );
        assert!(found.is_empty(), "{found:?}");
    }
}
