//! Declarations that are visible across files, so that the consistency rules can check uses of
//! package declarations and entity interfaces declared in other files of the same run.

use std::collections::HashMap;

use vhdl_syntax::syntax::{NodeKind as N, SyntaxNode};
use vhdl_syntax::tokens::TokenKind as T;

use super::select::{
    child, ident, ident_list, interface_idents, interface_list, subprogram_spec, text,
};
use crate::Parsed;

/// Declarations collected from a set of files.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Project {
    /// Package (lower case) → (consistency rule, declared spelling).
    packages: HashMap<String, Vec<(String, String)>>,
    /// Entity (lower case) → (generics, ports) as declared.
    entities: HashMap<String, (Vec<String>, Vec<String>)>,
}

/// The names a package item declares, with the consistency rule that checks their uses.
fn declarations(item: &SyntaxNode, out: &mut Vec<(String, String)>) {
    let (rule, names) = match item.kind() {
        N::SignalDeclaration => ("signal_014", ident_list(item)),
        N::ConstantDeclaration => ("constant_013", ident_list(item)),
        N::VariableDeclaration => ("variable_011", ident_list(item)),
        N::FullTypeDeclaration | N::IncompleteTypeDeclaration => {
            ("type_014", ident(item).into_iter().collect())
        }
        N::SubtypeDeclaration => ("subtype_002", ident(item).into_iter().collect()),
        N::AliasDeclaration => ("alias_declaration_503", ident(item).into_iter().collect()),
        N::SubprogramDeclaration => {
            let Some(spec) = subprogram_spec(item) else {
                return;
            };
            let rule = if spec.kind() == N::FunctionSpecification {
                "function_010"
            } else {
                "procedure_507"
            };
            (rule, ident(&spec).into_iter().collect())
        }
        _ => return,
    };
    out.extend(names.iter().map(|t| (rule.to_owned(), text(t))));
    let literals =
        child(item, N::EnumerationTypeDefinition).and_then(|d| child(&d, N::EnumerationList));
    if let Some(list) = literals {
        out.extend(
            list.children_with_tokens()
                .filter_map(|c| c.as_token())
                .filter(|t| t.kind() == T::Identifier)
                .map(|t| ("type_501".to_owned(), text(&t))),
        );
    }
}

fn interface(entity: &SyntaxNode, clause: N) -> Vec<String> {
    child(entity, N::EntityHeader)
        .and_then(|h| child(&h, clause))
        .and_then(|c| interface_list(&c))
        .map(|l| interface_idents(&l).iter().map(|(t, _)| text(t)).collect())
        .unwrap_or_default()
}

impl Project {
    pub fn new() -> Project {
        Project::default()
    }

    /// Collect the package declarations and entity interfaces of a snapshot.
    pub fn add(&mut self, parsed: &Parsed) {
        let mut stack = vec![parsed.root().clone()];
        while let Some(n) = stack.pop() {
            match n.kind() {
                N::PackageDeclaration => {
                    let Some(name) = child(&n, N::PackagePreamble).and_then(|p| ident(&p)) else {
                        continue;
                    };
                    let decls = self
                        .packages
                        .entry(text(&name).to_ascii_lowercase())
                        .or_default();
                    for part in n
                        .children()
                        .filter(|c| c.kind() == N::PackageDeclarativePart)
                    {
                        for item in part.children() {
                            declarations(&item, decls);
                        }
                    }
                }
                N::EntityDeclaration => {
                    if let Some(name) =
                        child(&n, N::EntityDeclarationPreamble).and_then(|p| ident(&p))
                    {
                        self.entities.insert(
                            text(&name).to_ascii_lowercase(),
                            (
                                interface(&n, N::GenericClause),
                                interface(&n, N::PortClause),
                            ),
                        );
                    }
                }
                _ => stack.extend(n.children()),
            }
        }
    }

    /// Combine two projects.
    #[must_use]
    pub fn merge(mut self, other: Project) -> Project {
        for (k, v) in other.packages {
            self.packages.entry(k).or_default().extend(v);
        }
        self.entities.extend(other.entities);
        self
    }

    /// Names that `rule` checks, declared in `package` (lower case).
    pub(crate) fn package_names<'a>(
        &'a self,
        package: &str,
        rule: &'a str,
    ) -> impl Iterator<Item = &'a str> + 'a {
        self.packages
            .get(package)
            .into_iter()
            .flatten()
            .filter(move |(r, _)| *r == rule)
            .map(|(_, name)| name.as_str())
    }

    /// Generics (`ports == false`) or ports of an entity (lower case), as declared.
    pub(crate) fn entity_interface(&self, entity: &str, ports: bool) -> Option<&[String]> {
        self.entities
            .get(entity)
            .map(|(g, p)| if ports { p.as_slice() } else { g.as_slice() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_package_and_entity_declarations() {
        let src = "package Pkg is\n  constant C_Width : natural := 8;\n  type State_T is (Idle, Busy);\n  function Add (a : integer) return integer;\nend package;\nentity Ent is\n  generic (G_N : natural);\n  port (Clk : in bit);\nend entity;\n";
        let mut project = Project::new();
        project.add(&Parsed::new(src.as_bytes().to_vec()));
        let names = |rule| project.package_names("pkg", rule).collect::<Vec<_>>();
        assert_eq!(names("constant_013"), ["C_Width"]);
        assert_eq!(names("type_014"), ["State_T"]);
        assert_eq!(names("type_501"), ["Idle", "Busy"]);
        assert_eq!(names("function_010"), ["Add"]);
        assert_eq!(
            project.entity_interface("ent", true),
            Some(&["Clk".to_owned()][..])
        );
        assert_eq!(
            project.entity_interface("ent", false),
            Some(&["G_N".to_owned()][..])
        );
    }
}
