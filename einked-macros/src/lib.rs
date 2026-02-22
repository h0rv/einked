use proc_macro::TokenStream;
use proc_macro2::{Delimiter, Punct, Spacing, TokenStream as TokenStream2, TokenTree};
use quote::quote;

#[derive(Debug, Clone, Copy)]
enum RefreshMode {
    Full,
    Partial,
    Fast,
}

#[derive(Debug)]
struct DecoratedNode {
    refresh: Option<RefreshMode>,
    node: Node,
}

#[derive(Debug)]
enum Node {
    VStack {
        gap: TokenStream2,
        pad: TokenStream2,
        children: Vec<DecoratedNode>,
    },
    HStack {
        gap: TokenStream2,
        pad: TokenStream2,
        children: Vec<DecoratedNode>,
    },
    Label {
        value: TokenStream2,
    },
    Paragraph {
        value: TokenStream2,
    },
    StatusBar {
        left: TokenStream2,
        right: TokenStream2,
    },
    Divider,
    Spacer,
}

#[proc_macro]
pub fn ui(input: TokenStream) -> TokenStream {
    let tokens: Vec<TokenTree> = TokenStream2::from(input).into_iter().collect();
    let mut idx = 0usize;

    let nodes = match parse_nodes(&tokens, &mut idx) {
        Ok(nodes) => nodes,
        Err(err) => return compile_error(err),
    };

    let expanded = emit_nodes(&nodes);
    quote!({ #expanded }).into()
}

fn parse_nodes(tokens: &[TokenTree], idx: &mut usize) -> Result<Vec<DecoratedNode>, String> {
    let mut nodes = Vec::new();
    while *idx < tokens.len() {
        skip_separators(tokens, idx);
        if *idx >= tokens.len() {
            break;
        }
        nodes.push(parse_decorated_node(tokens, idx)?);
        skip_separators(tokens, idx);
    }
    Ok(nodes)
}

fn parse_decorated_node(tokens: &[TokenTree], idx: &mut usize) -> Result<DecoratedNode, String> {
    let mut refresh = None;

    while is_refresh_attribute_start(tokens, *idx) {
        let mode = parse_refresh_attribute(tokens, idx)?;
        if refresh.replace(mode).is_some() {
            return Err("duplicate `#[refresh = ...]` annotation on the same node".into());
        }
        skip_separators(tokens, idx);
    }

    let node = parse_node(tokens, idx)?;
    Ok(DecoratedNode { refresh, node })
}

fn parse_node(tokens: &[TokenTree], idx: &mut usize) -> Result<Node, String> {
    let name = match tokens.get(*idx) {
        Some(TokenTree::Ident(ident)) => {
            *idx += 1;
            ident.to_string()
        }
        other => return Err(format!("expected node identifier, found {:?}", other)),
    };

    match name.as_str() {
        "VStack" => parse_stack(tokens, idx, true),
        "HStack" => parse_stack(tokens, idx, false),
        "Label" => parse_label_like(tokens, idx, "Label", true),
        "Paragraph" => parse_label_like(tokens, idx, "Paragraph", false),
        "StatusBar" => parse_status_bar(tokens, idx),
        "Divider" => Ok(Node::Divider),
        "Spacer" => Ok(Node::Spacer),
        _ => Err(format!(
            "unsupported ui! node `{}` (supported: VStack, HStack, Label, Paragraph, StatusBar, Divider, Spacer)",
            name
        )),
    }
}

fn parse_stack(tokens: &[TokenTree], idx: &mut usize, vertical: bool) -> Result<Node, String> {
    let mut gap = quote!(0u16);
    let mut pad = quote!(0u16);

    loop {
        skip_separators(tokens, idx);
        match tokens.get(*idx) {
            Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Brace => {
                *idx += 1;
                let inner: Vec<TokenTree> = group.stream().into_iter().collect();
                let mut inner_idx = 0usize;
                let children = parse_nodes(&inner, &mut inner_idx)?;
                return Ok(if vertical {
                    Node::VStack { gap, pad, children }
                } else {
                    Node::HStack { gap, pad, children }
                });
            }
            Some(TokenTree::Ident(attr)) => {
                let attr_name = attr.to_string();
                *idx += 1;
                expect_equals(tokens, idx)?;
                let value = parse_attr_value(tokens, idx)?;
                match attr_name.as_str() {
                    "gap" => gap = value,
                    "pad" => pad = value,
                    _ => {
                        return Err(format!(
                            "unsupported {} attribute `{}`",
                            if vertical { "VStack" } else { "HStack" },
                            attr_name
                        ));
                    }
                }
            }
            other => {
                return Err(format!(
                    "expected {} attribute or body, found {:?}",
                    if vertical { "VStack" } else { "HStack" },
                    other
                ));
            }
        }
    }
}

fn parse_label_like(
    tokens: &[TokenTree],
    idx: &mut usize,
    name: &str,
    is_label: bool,
) -> Result<Node, String> {
    match tokens.get(*idx) {
        Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis => {
            *idx += 1;
            if is_label {
                Ok(Node::Label {
                    value: group.stream(),
                })
            } else {
                Ok(Node::Paragraph {
                    value: group.stream(),
                })
            }
        }
        other => Err(format!("{} requires parentheses, found {:?}", name, other)),
    }
}

fn parse_status_bar(tokens: &[TokenTree], idx: &mut usize) -> Result<Node, String> {
    let Some(TokenTree::Group(group)) = tokens.get(*idx) else {
        return Err(
            "StatusBar requires a braced body like `StatusBar { left: ..., right: ... }`".into(),
        );
    };

    if group.delimiter() != Delimiter::Brace {
        return Err(format!(
            "StatusBar requires braces, found {:?}",
            group.delimiter()
        ));
    }

    *idx += 1;
    let inner: Vec<TokenTree> = group.stream().into_iter().collect();
    let mut inner_idx = 0usize;
    let mut left = None;
    let mut right = None;

    while inner_idx < inner.len() {
        skip_separators(&inner, &mut inner_idx);
        if inner_idx >= inner.len() {
            break;
        }

        let field = match inner.get(inner_idx) {
            Some(TokenTree::Ident(ident)) => {
                inner_idx += 1;
                ident.to_string()
            }
            other => {
                return Err(format!(
                    "StatusBar expected `left` or `right`, found {:?}",
                    other
                ));
            }
        };

        expect_colon(&inner, &mut inner_idx)?;
        let value = parse_field_value(&inner, &mut inner_idx)?;

        match field.as_str() {
            "left" => {
                if left.replace(value).is_some() {
                    return Err("StatusBar field `left` specified more than once".into());
                }
            }
            "right" => {
                if right.replace(value).is_some() {
                    return Err("StatusBar field `right` specified more than once".into());
                }
            }
            _ => return Err(format!("StatusBar does not support field `{}`", field)),
        }

        skip_separators(&inner, &mut inner_idx);
    }

    let Some(left) = left else {
        return Err("StatusBar requires a `left: ...` field".into());
    };
    let Some(right) = right else {
        return Err("StatusBar requires a `right: ...` field".into());
    };

    Ok(Node::StatusBar { left, right })
}

fn parse_field_value(tokens: &[TokenTree], idx: &mut usize) -> Result<TokenStream2, String> {
    let start = *idx;
    while *idx < tokens.len() {
        match tokens.get(*idx) {
            Some(TokenTree::Punct(p)) if p.as_char() == ',' || p.as_char() == ';' => break,
            Some(_) => *idx += 1,
            None => break,
        }
    }

    if *idx == start {
        return Err("expected field value".into());
    }

    let value: TokenStream2 = tokens[start..*idx].iter().cloned().collect();
    Ok(value)
}

fn parse_attr_value(tokens: &[TokenTree], idx: &mut usize) -> Result<TokenStream2, String> {
    match tokens.get(*idx) {
        Some(TokenTree::Literal(lit)) => {
            *idx += 1;
            Ok(quote!(#lit))
        }
        Some(TokenTree::Ident(ident)) => {
            *idx += 1;
            Ok(quote!(#ident))
        }
        Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis => {
            *idx += 1;
            let content = group.stream();
            Ok(quote!((#content)))
        }
        other => Err(format!("expected attribute value, found {:?}", other)),
    }
}

fn is_refresh_attribute_start(tokens: &[TokenTree], idx: usize) -> bool {
    matches!(tokens.get(idx), Some(TokenTree::Punct(p)) if p.as_char() == '#')
        && matches!(tokens.get(idx + 1), Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Bracket)
}

fn parse_refresh_attribute(tokens: &[TokenTree], idx: &mut usize) -> Result<RefreshMode, String> {
    match tokens.get(*idx) {
        Some(TokenTree::Punct(p)) if p.as_char() == '#' => {
            *idx += 1;
        }
        other => {
            return Err(format!(
                "expected refresh attribute start `#`, found {:?}",
                other
            ));
        }
    }

    let bracket = match tokens.get(*idx) {
        Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Bracket => {
            *idx += 1;
            group.clone()
        }
        other => {
            return Err(format!(
                "expected `[...]` after `#` for attribute, found {:?}",
                other
            ));
        }
    };

    let inner: Vec<TokenTree> = bracket.stream().into_iter().collect();
    let mut inner_idx = 0usize;

    let attr_name = match inner.get(inner_idx) {
        Some(TokenTree::Ident(ident)) => {
            inner_idx += 1;
            ident.to_string()
        }
        other => {
            return Err(format!(
                "expected attribute name in `#[...]`, found {:?}",
                other
            ));
        }
    };

    if attr_name != "refresh" {
        return Err(format!(
            "unsupported ui! attribute `#{}` (only `#[refresh = ...]` is supported)",
            attr_name
        ));
    }

    expect_equals(&inner, &mut inner_idx)?;
    let mode = parse_refresh_mode(&inner, &mut inner_idx)?;
    skip_separators(&inner, &mut inner_idx);

    if inner_idx != inner.len() {
        return Err("unexpected extra tokens in `#[refresh = ...]` attribute".into());
    }

    Ok(mode)
}

fn parse_refresh_mode(tokens: &[TokenTree], idx: &mut usize) -> Result<RefreshMode, String> {
    let mode = match tokens.get(*idx) {
        Some(TokenTree::Ident(ident)) => {
            *idx += 1;
            ident.to_string()
        }
        other => {
            return Err(format!(
                "expected refresh mode identifier (Full, Partial, Fast), found {:?}",
                other
            ));
        }
    };

    match mode.as_str() {
        "Full" => Ok(RefreshMode::Full),
        "Partial" => Ok(RefreshMode::Partial),
        "Fast" => Ok(RefreshMode::Fast),
        _ => Err(format!(
            "unsupported refresh mode `{}` (expected Full, Partial, or Fast)",
            mode
        )),
    }
}

fn emit_nodes(nodes: &[DecoratedNode]) -> TokenStream2 {
    let emitted: Vec<TokenStream2> = nodes.iter().map(emit_decorated_node).collect();
    quote!(#(#emitted)*)
}

fn emit_decorated_node(node: &DecoratedNode) -> TokenStream2 {
    let emitted = emit_node(&node.node);
    if let Some(refresh) = node.refresh {
        let mode = emit_refresh_mode(refresh);
        quote! {
            ui.with_refresh(#mode, |ui| {
                #emitted
            });
        }
    } else {
        emitted
    }
}

fn emit_refresh_mode(mode: RefreshMode) -> TokenStream2 {
    match mode {
        RefreshMode::Full => quote!(::einked::dsl::RefreshMode::Full),
        RefreshMode::Partial => quote!(::einked::dsl::RefreshMode::Partial),
        RefreshMode::Fast => quote!(::einked::dsl::RefreshMode::Fast),
    }
}

fn emit_node(node: &Node) -> TokenStream2 {
    match node {
        Node::VStack { gap, pad, children } => {
            let inner = emit_nodes(children);
            quote! {
                ui.vstack(::einked::dsl::StackOpts { gap: (#gap) as u16, pad: (#pad) as u16 }, |ui| {
                    #inner
                });
            }
        }
        Node::HStack { gap, pad, children } => {
            let inner = emit_nodes(children);
            quote! {
                ui.hstack(::einked::dsl::StackOpts { gap: (#gap) as u16, pad: (#pad) as u16 }, |ui| {
                    #inner
                });
            }
        }
        Node::Label { value } => quote!(ui.label(#value);),
        Node::Paragraph { value } => quote!(ui.paragraph(#value);),
        Node::StatusBar { left, right } => quote!(ui.status_bar(#left, #right);),
        Node::Divider => quote!(ui.divider();),
        Node::Spacer => quote!(ui.spacer();),
    }
}

fn skip_separators(tokens: &[TokenTree], idx: &mut usize) {
    while let Some(TokenTree::Punct(p)) = tokens.get(*idx) {
        if p.as_char() == ',' || p.as_char() == ';' {
            *idx += 1;
        } else {
            break;
        }
    }
}

fn expect_equals(tokens: &[TokenTree], idx: &mut usize) -> Result<(), String> {
    match tokens.get(*idx) {
        Some(TokenTree::Punct(Punct { .. })) if is_equals(tokens.get(*idx)) => {
            *idx += 1;
            Ok(())
        }
        other => Err(format!("expected `=`, found {:?}", other)),
    }
}

fn expect_colon(tokens: &[TokenTree], idx: &mut usize) -> Result<(), String> {
    match tokens.get(*idx) {
        Some(TokenTree::Punct(p)) if p.as_char() == ':' && p.spacing() == Spacing::Alone => {
            *idx += 1;
            Ok(())
        }
        other => Err(format!("expected `:`, found {:?}", other)),
    }
}

fn is_equals(token: Option<&TokenTree>) -> bool {
    matches!(token, Some(TokenTree::Punct(p)) if p.as_char() == '=' && p.spacing() == Spacing::Alone)
}

fn compile_error(message: String) -> TokenStream {
    quote!(compile_error!(#message);).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    fn parse(input: TokenStream2) -> Result<Vec<DecoratedNode>, String> {
        let tokens: Vec<TokenTree> = input.into_iter().collect();
        let mut idx = 0usize;
        parse_nodes(&tokens, &mut idx)
    }

    #[test]
    fn parses_new_nodes_and_refresh_annotation() {
        let nodes = parse(quote! {
            #[refresh = Full]
            Divider
            StatusBar { left: "L", right: "R" }
            Paragraph("body")
        })
        .expect("parse should succeed");

        assert_eq!(nodes.len(), 3);
        assert!(matches!(nodes[0].refresh, Some(RefreshMode::Full)));
        assert!(matches!(nodes[0].node, Node::Divider));
        assert!(matches!(nodes[1].node, Node::StatusBar { .. }));
        assert!(matches!(nodes[2].node, Node::Paragraph { .. }));
    }

    #[test]
    fn emits_expected_runtime_calls_for_new_nodes() {
        let nodes = parse(quote! {
            #[refresh = Partial]
            StatusBar { left: "L", right: "R" }
            Divider
            Paragraph("P")
        })
        .expect("parse should succeed");

        let out = emit_nodes(&nodes).to_string();
        assert!(out.contains("ui . with_refresh"));
        assert!(out.contains(":: einked :: dsl :: RefreshMode :: Partial"));
        assert!(out.contains("ui . status_bar"));
        assert!(out.contains("ui . divider"));
        assert!(out.contains("ui . paragraph"));
    }

    #[test]
    fn rejects_invalid_refresh_mode_with_readable_message() {
        let err = parse(quote! {
            #[refresh = Turbo]
            Label("x")
        })
        .expect_err("parse should fail");

        assert!(err.contains("unsupported refresh mode `Turbo`"));
        assert!(err.contains("Full, Partial, or Fast"));
    }

    #[test]
    fn rejects_status_bar_without_required_fields() {
        let err = parse(quote! {
            StatusBar { left: "only-left" }
        })
        .expect_err("parse should fail");

        assert!(err.contains("StatusBar requires a `right: ...` field"));
    }
}
