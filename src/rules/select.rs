//! Syntax-tree selectors shared by the naming and case rules.

use vhdl_syntax::syntax::{NodeKind as N, SyntaxNode, SyntaxToken};
use vhdl_syntax::tokens::{Keyword as Kw, TokenKind as T};

use super::Context;

pub(crate) fn child(n: &SyntaxNode, kind: N) -> Option<SyntaxNode> {
    n.children().find(|c| c.kind() == kind)
}

pub(crate) fn children(n: &SyntaxNode, kind: N) -> impl Iterator<Item = SyntaxNode> + '_ {
    n.children().filter(move |c| c.kind() == kind)
}

/// Direct child tokens of a node.
pub(crate) fn tokens(n: &SyntaxNode) -> impl Iterator<Item = SyntaxToken> + '_ {
    n.children_with_tokens().filter_map(|c| c.as_token())
}

/// The first identifier that is a direct child of `n`.
pub(crate) fn ident(n: &SyntaxNode) -> Option<SyntaxToken> {
    tokens(n).find(|t| t.kind() == T::Identifier)
}

/// Identifiers of an `IdentifierList` child.
pub(crate) fn ident_list(n: &SyntaxNode) -> Vec<SyntaxToken> {
    child(n, N::IdentifierList)
        .map(|l| tokens(&l).filter(|t| t.kind() == T::Identifier).collect())
        .unwrap_or_default()
}

/// A token's text, as a `String`.
pub(crate) fn text(t: &SyntaxToken) -> String {
    String::from_utf8_lossy(t.text().as_bytes()).into_owned()
}

/// Every token of `n`, including those of its children.
pub(crate) fn all_tokens(n: &SyntaxNode) -> Vec<SyntaxToken> {
    let mut out = Vec::new();
    crate::collect_tokens(n, &mut out);
    out
}

/// The label of a statement (also looked up in a loop preamble).
pub(crate) fn label(n: &SyntaxNode) -> Option<SyntaxToken> {
    child(n, N::StmtLabel)
        .or_else(|| child(n, N::LoopStatementPreamble).and_then(|p| child(&p, N::StmtLabel)))
        .and_then(|l| ident(&l))
}

/// The identifier after `end` in an epilogue child of `n`.
pub(crate) fn end_ident(n: &SyntaxNode, epilogue: N) -> Option<SyntaxToken> {
    let e = child(n, epilogue)?;
    let mut all = Vec::new();
    crate::collect_tokens(&e, &mut all);
    all.into_iter().find(|t| t.kind() == T::Identifier)
}

/// Identifiers that make up a name: the prefix and selected suffixes, not the contents of
/// parenthesized parts or attributes.
pub(crate) fn name_idents(name: &SyntaxNode) -> Vec<SyntaxToken> {
    name.children()
        .filter(|p| matches!(p.kind(), N::NameDesignatorPrefix | N::SelectedName))
        .flat_map(|p| {
            tokens(&p)
                .filter(|t| t.kind() == T::Identifier)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Identifiers (with their mode) declared by the interface declarations of a list.
pub(crate) fn interface_idents(list: &SyntaxNode) -> Vec<(SyntaxToken, Option<Kw>)> {
    let mut out = Vec::new();
    for decl in children(list, N::InterfaceObjectDeclaration)
        .chain(children(list, N::InterfaceFileDeclaration))
    {
        let mode = tokens(&decl).find_map(|t| match t.kind() {
            T::Keyword(k @ (Kw::In | Kw::Out | Kw::Inout | Kw::Buffer | Kw::Linkage)) => Some(k),
            _ => None,
        });
        out.extend(ident_list(&decl).into_iter().map(|t| (t, mode)));
    }
    out
}

/// The interface list of a generic clause, port clause or parameter list.
pub(crate) fn interface_list(n: &SyntaxNode) -> Option<SyntaxNode> {
    match n.kind() {
        N::GenericClause | N::PortClause | N::SubprogramHeaderGenericClause => {
            child(n, N::InterfaceList)
        }
        N::ParameterList => {
            child(n, N::ParenthesizedInterfaceList).and_then(|p| child(&p, N::InterfaceList))
        }
        _ => None,
    }
}

/// Identifiers declared by all clauses of `kind` (generic or port clauses).
pub(crate) fn clause_idents(cx: &Context<'_>, kind: N) -> Vec<(SyntaxToken, Option<Kw>)> {
    cx.nodes(kind)
        .iter()
        .filter_map(interface_list)
        .flat_map(|l| interface_idents(&l))
        .collect()
}

/// Formal names (first identifier of each formal part) of an association list inside `n`.
pub(crate) fn formals(n: &SyntaxNode) -> Vec<SyntaxToken> {
    let Some(list) = child(n, N::AssociationList) else {
        return Vec::new();
    };
    children(&list, N::AssociationElement)
        .filter_map(|e| child(&e, N::Formal))
        .filter_map(|f| child(&f, N::Name))
        .filter_map(|name| name_idents(&name).into_iter().next())
        .collect()
}

/// The specification (function or procedure) of a subprogram body or declaration.
pub(crate) fn subprogram_spec(n: &SyntaxNode) -> Option<SyntaxNode> {
    let spec = match n.kind() {
        N::SubprogramBody => child(n, N::SubprogramBodyPreamble)?.first_child()?,
        _ => n.first_child()?,
    };
    matches!(
        spec.kind(),
        N::FunctionSpecification | N::ProcedureSpecification
    )
    .then_some(spec)
}

/// All subprogram specifications of the given kind (in declarations and bodies).
pub(crate) fn specs(cx: &Context<'_>, kind: N) -> Vec<SyntaxNode> {
    cx.nodes(N::SubprogramBody)
        .iter()
        .chain(cx.nodes(N::SubprogramDeclaration))
        .filter_map(subprogram_spec)
        .filter(|s| s.kind() == kind)
        .collect()
}

/// Subprogram bodies whose specification has the given kind.
pub(crate) fn bodies(cx: &Context<'_>, kind: N) -> Vec<SyntaxNode> {
    cx.nodes(N::SubprogramBody)
        .iter()
        .filter(|b| subprogram_spec(b).is_some_and(|s| s.kind() == kind))
        .cloned()
        .collect()
}

/// The parameters of a subprogram specification.
pub(crate) fn parameters(spec: &SyntaxNode) -> Vec<SyntaxToken> {
    child(spec, N::ParameterList)
        .and_then(|p| interface_list(&p))
        .map(|l| interface_idents(&l).into_iter().map(|(t, _)| t).collect())
        .unwrap_or_default()
}

const GENERATES: [N; 3] = [
    N::ForGenerateStatement,
    N::IfGenerateStatement,
    N::CaseGenerateStatement,
];

/// The labels of all generate statements.
pub(crate) fn generate_labels(cx: &Context<'_>) -> Vec<SyntaxToken> {
    GENERATES
        .iter()
        .flat_map(|k| cx.nodes(*k))
        .filter_map(label)
        .collect()
}

/// The labels after `end generate`.
pub(crate) fn generate_end_labels(cx: &Context<'_>) -> Vec<SyntaxToken> {
    GENERATES
        .iter()
        .flat_map(|k| cx.nodes(*k))
        .filter_map(|n| end_ident(n, N::GenerateEpilogue))
        .collect()
}

/// Enumeration literals (identifiers only).
pub(crate) fn enum_literals(cx: &Context<'_>) -> Vec<SyntaxToken> {
    cx.nodes(N::EnumerationList)
        .iter()
        .flat_map(|l| {
            tokens(l)
                .filter(|t| t.kind() == T::Identifier)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Designators of subprogram specifications of the given kind.
pub(crate) fn designators(cx: &Context<'_>, kind: N) -> Vec<SyntaxToken> {
    specs(cx, kind).iter().filter_map(ident).collect()
}

/// Identifiers declared by all declarations of `kind` that have an identifier list.
pub(crate) fn declared(cx: &Context<'_>, kind: N) -> Vec<SyntaxToken> {
    cx.nodes(kind).iter().flat_map(ident_list).collect()
}

/// Identifiers directly inside all nodes of `kind`.
pub(crate) fn idents_of(cx: &Context<'_>, kind: N) -> Vec<SyntaxToken> {
    cx.nodes(kind).iter().filter_map(ident).collect()
}

/// Labels of all statements of `kind`.
pub(crate) fn labels_of(cx: &Context<'_>, kind: N) -> Vec<SyntaxToken> {
    cx.nodes(kind).iter().filter_map(label).collect()
}

/// Tokens in positions that declare a name or a label (excluded from use-site checks).
pub(crate) fn is_declaration_position(t: &SyntaxToken) -> bool {
    matches!(
        t.parent().kind(),
        N::IdentifierList
            | N::StmtLabel
            | N::EnumerationList
            | N::ArchitecturePreamble
            | N::EntityDeclarationPreamble
            | N::PackagePreamble
            | N::PackageBodyPreamble
            | N::ComponentDeclarationPreamble
            | N::ContextDeclarationPreamble
            | N::FullTypeDeclaration
            | N::IncompleteTypeDeclaration
            | N::SubtypeDeclaration
            | N::AliasDeclaration
            | N::FunctionSpecification
            | N::ProcedureSpecification
            | N::ParameterSpecification
            | N::PackageInstantiationPreamble
            | N::SubprogramInstantiationDeclarationPreamble
            | N::InterfaceIncompleteTypeDeclaration
            | N::AttributeDeclaration
            | N::PrimaryUnitDeclaration
            | N::SecondaryUnitDeclaration
    )
}

/// Whether a use-site token refers to something other than a plain name in scope: a selected
/// suffix (`record.field`, `lib.pkg`), an attribute designator, or the formal of an association.
pub(crate) fn is_qualified_position(t: &SyntaxToken) -> bool {
    let parent = t.parent();
    match parent.kind() {
        N::SelectedName | N::AttributeName => true,
        N::NameDesignatorPrefix => parent.ancestors().take(3).any(|a| a.kind() == N::Formal),
        _ => false,
    }
}
