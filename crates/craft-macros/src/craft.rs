use crate::utils::salvo_crate;
use proc_macro2::{Span, TokenStream};
use quote::{quote, ToTokens};
use regex::Regex;
use syn::parse::Parser;
use syn::{
    parse_quote, Attribute, FnArg, Generics, Ident, ImplItem, ImplItemFn, Item, Token, Type,
};

pub(crate) fn generate(input: Item) -> syn::Result<TokenStream> {
    match input {
        Item::Impl(mut item_impl) => {
            for item in &mut item_impl.items {
                if let ImplItem::Fn(method) = item {
                    rewrite_method(
                        item_impl.generics.clone(),
                        item_impl.self_ty.clone(),
                        method,
                    )?;
                }
            }
            Ok(item_impl.into_token_stream())
        }
        Item::Fn(_) => Ok(input.into_token_stream()),
        _ => Err(syn::Error::new_spanned(
            input,
            "#[craft] must added to `impl`",
        )),
    }
}

const REGEX_STR: &'static str =
    r#"(?s)#\s*\[\s*craft\s*\(\s*(?P<name>handler|endpoint)\s*(?P<content>\(.*\))?\s*\)\s*\]"#;

fn take_method_macro(item_fn: &mut ImplItemFn) -> syn::Result<Option<Attribute>> {
    let mut index: Option<usize> = None;
    let mut new_attr: Option<Attribute> = None;
    for (idx, attr) in &mut item_fn.attrs.iter().enumerate() {
        if !(match attr.path().segments.last() {
            Some(segment) => segment.ident == "craft",
            None => false,
        }) {
            continue;
        }
        let re = Regex::new(REGEX_STR).unwrap();
        let attr_str = attr.to_token_stream().to_string().trim().to_owned();
        if let Some(caps) = re.captures(&attr_str) {
            if let Some(name) = caps.name("name") {
                let name = name.as_str();
                let content = caps
                    .name("content")
                    .map(|c| c.as_str().to_string())
                    .unwrap_or_default();
                let ts: TokenStream = match name {
                    "handler" => format!("#[{}::{name}{content}]", salvo_crate()).parse()?,
                    "endpoint" => format!("#[{}::oapi::{name}{content}]", salvo_crate()).parse()?,
                    _ => {
                        unreachable!()
                    }
                };
                new_attr = Attribute::parse_outer.parse2(ts)?.into_iter().next();
                index = Some(idx);
                continue;
            }
        }
        return Err(syn::Error::new_spanned(
                item_fn,
                format!("The attribute macro `{attr_str}` on a method must be filled with sub-attributes, such as '#[craft(handler)]', '#[craft(endpoint)]', or '#[craft(endpoint(...))]'."),
            ));
    }
    if let Some(index) = index {
        item_fn.attrs.remove(index);
        return Ok(new_attr);
    }
    Ok(None)
}

enum FnReceiver {
    None,
    Ref,
    Arc,
}

impl FnReceiver {
    fn from_method(method: &ImplItemFn) -> syn::Result<Self> {
        let Some(recv) = method.sig.receiver() else {
            return Ok(Self::None);
        };
        let ty = recv.ty.to_token_stream().to_string().replace(" ", "");
        match ty.as_str() {
            "&Self" => Ok(Self::Ref),
            "Arc<Self>" | "&Arc<Self>" => Ok(Self::Arc),
            _ => {
                if ty.ends_with("::Arc<Self>") {
                    Ok(Self::Arc)
                } else {
                    Err(syn::Error::new_spanned(
                        method,
                        "#[craft] method receiver must be '&self', 'Arc<Self>' or '&Arc<Self>'",
                    ))
                }
            }
        }
    }
}

fn rewrite_method(
    mut impl_generics: Generics,
    self_ty: Box<Type>,
    method: &mut ImplItemFn,
) -> syn::Result<()> {
    let Some(macro_attr) = take_method_macro(method)? else {
        return Ok(());
    };
    method.sig.asyncness = Some(Token![async](Span::call_site()));
    let salvo = salvo_crate();
    let handler = quote!(#salvo::Handler);
    let method_name = method.sig.ident.clone();
    let vis = method.vis.clone();
    let mut attrs = method.attrs.clone();
    let mut new_method: ImplItemFn = match FnReceiver::from_method(method)? {
        FnReceiver::None => {
            method.attrs.push(macro_attr);
            parse_quote! {
                #vis fn #method_name() -> impl #handler {

                    #method

                    #method_name
                }
            }
        }
        style => {
            let (receiver, output) = match style {
                FnReceiver::Ref => (quote!(&self), quote!(::std::sync::Arc::new(self.clone()))),
                FnReceiver::Arc => (quote!(self: &::std::sync::Arc<Self>), quote!(self.clone())),
                _ => unreachable!(),
            };
            method.sig.inputs[0] = FnArg::Receiver(parse_quote!(&self));
            method.sig.ident = Ident::new("handle", Span::call_site());
            let where_clause = impl_generics.make_where_clause().clone();
            parse_quote! {
                #vis fn #method_name(#receiver) -> impl #handler {
                    pub struct handle #impl_generics(::std::sync::Arc<#self_ty>) #where_clause;
                    use ::std::ops::Deref;
                    impl #impl_generics Deref for handle #impl_generics #where_clause{
                        type Target = #self_ty;

                        fn deref(&self) -> &Self::Target {
                            &self.0
                        }
                    }
                    #[allow(unused_imports)]
                    use ::std::ops::Deref as _;
                    #macro_attr
                    impl #impl_generics handle #impl_generics #where_clause{
                        #method
                    }
                    handle(#output)
                }
            }
        }
    };
    new_method.attrs.append(&mut attrs);
    *method = new_method;
    Ok(())
}
