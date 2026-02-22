use proc_macro::TokenStream;
use proc_macro2::{Delimiter, Punct, Spacing, TokenStream as TokenStream2, TokenTree};
use quote::quote;

#[derive(Debug)]
enum Node {
    VStack {
        gap: TokenStream2,
        pad: TokenStream2,
        children: Vec<Node>,
    },
    HStack {
        gap: TokenStream2,
        pad: TokenStream2,
        children: Vec<Node>,
    },
    Label {
        value: TokenStream2,
    },
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

fn parse_nodes(tokens: &[TokenTree], idx: &mut usize) -> Result<Vec<Node>, String> {
    let mut nodes = Vec::new();
    while *idx < tokens.len() {
        skip_separators(tokens, idx);
        if *idx >= tokens.len() {
            break;
        }
        nodes.push(parse_node(tokens, idx)?);
        skip_separators(tokens, idx);
    }
    Ok(nodes)
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
        "Label" => parse_label(tokens, idx),
        "Spacer" => Ok(Node::Spacer),
        _ => Err(format!("unsupported ui! node `{}`", name)),
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
                        ))
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

fn parse_label(tokens: &[TokenTree], idx: &mut usize) -> Result<Node, String> {
    match tokens.get(*idx) {
        Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis => {
            *idx += 1;
            Ok(Node::Label {
                value: group.stream(),
            })
        }
        other => Err(format!("Label requires parentheses, found {:?}", other)),
    }
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

fn emit_nodes(nodes: &[Node]) -> TokenStream2 {
    let emitted: Vec<TokenStream2> = nodes.iter().map(emit_node).collect();
    quote!(#(#emitted)*)
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

fn is_equals(token: Option<&TokenTree>) -> bool {
    matches!(token, Some(TokenTree::Punct(p)) if p.as_char() == '=' && p.spacing() == Spacing::Alone)
}

fn compile_error(message: String) -> TokenStream {
    quote!(compile_error!(#message);).into()
}
