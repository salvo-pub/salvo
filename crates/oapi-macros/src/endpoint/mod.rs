use proc_macro2::{Span, TokenStream};
use quote::{quote, ToTokens};
use syn::{Ident, ImplItem, Item, Pat, ReturnType, Signature, Type};

use crate::doc_comment::CommentAttributes;
use crate::{omit_type_path_lifetimes, parse_input_type, InputType, Operation};

mod attr;
pub(crate) use attr::EndpointAttr;

fn metadata(oapi: &Ident, attr: EndpointAttr, name: &Ident, modifiers: Vec<TokenStream>) -> syn::Result<TokenStream> {
    let tfn = Ident::new(&format!("__salvo_oapi_type_id_{}", name), Span::call_site());
    let ofn = Ident::new(&format!("__salvo_oapi_operation_{}", name), Span::call_site());
    let opt = Operation::new(&attr);
    Ok(quote! {
        fn #tfn() -> ::std::any::TypeId {
            ::std::any::TypeId::of::<#name>()
        }
        fn #ofn() -> #oapi::oapi::Operation {
            let mut operation = #opt;
            #(#modifiers)*
            operation
        }
        #oapi::oapi::__private::inventory::submit! {
            #oapi::oapi::OperationRegistry::save(#tfn, #ofn)
        }
    })
}
pub(crate) fn generate(mut attr: EndpointAttr, input: Item) -> syn::Result<TokenStream> {
    let salvo = crate::salvo_crate();
    let oapi = crate::oapi_crate();
    match input {
        Item::Fn(mut item_fn) => {
            let attrs = &item_fn.attrs;
            let vis = &item_fn.vis;
            let sig = &mut item_fn.sig;
            let body = &item_fn.block;
            let name = &sig.ident;
            let docs = item_fn
                .attrs
                .iter()
                .filter(|attr| attr.path().is_ident("doc"))
                .cloned()
                .collect::<Vec<_>>();

            let sdef = quote! {
                #(#docs)*
                #[allow(non_camel_case_types)]
                #[derive(Debug)]
                #vis struct #name;
                impl #name {
                    #(#attrs)*
                    #sig {
                        #body
                    }
                }
            };

            attr.doc_comments = Some(CommentAttributes::from_attributes(attrs).0);
            attr.deprecated = attrs.iter().find_map(|attr| {
                if !matches!(attr.path().get_ident(), Some(ident) if &*ident.to_string() == "deprecated") {
                    None
                } else {
                    Some(true)
                }
            });

            let (hfn, modifiers) = handle_fn(&salvo, &oapi, sig)?;
            let meta = metadata(&oapi, attr, name, modifiers)?;
            Ok(quote! {
                #sdef
                #[#salvo::async_trait]
                impl #salvo::Handler for #name {
                    #hfn
                }
                #meta
            })
        }
        Item::Impl(item_impl) => {
            let attrs = &item_impl.attrs;

            attr.doc_comments = Some(CommentAttributes::from_attributes(attrs).0);
            attr.deprecated = attrs.iter().find_map(|attr| {
                if !matches!(attr.path().get_ident(), Some(ident) if &*ident.to_string() == "deprecated") {
                    None
                } else {
                    Some(true)
                }
            });

            let mut hmtd = None;
            for item in &item_impl.items {
                if let ImplItem::Fn(method) = item {
                    if method.sig.ident == Ident::new("handle", Span::call_site()) {
                        hmtd = Some(method);
                    }
                }
            }
            if hmtd.is_none() {
                return Err(syn::Error::new_spanned(item_impl.impl_token, "missing handle function"));
            }
            let hmtd = hmtd.unwrap();
            let (hfn, modifiers) = handle_fn(&salvo, &oapi, &hmtd.sig)?;
            let ty = &item_impl.self_ty;
            let (impl_generics, _, where_clause) = &item_impl.generics.split_for_impl();
            let name = Ident::new(&ty.to_token_stream().to_string(), Span::call_site());
            let meta = metadata(&oapi, attr, &name, modifiers)?;

            Ok(quote! {
                #item_impl
                #[#salvo::async_trait]
                impl #impl_generics #salvo::Handler for #ty #where_clause {
                    #hfn
                }
                #meta
            })
        }
        _ => Err(syn::Error::new_spanned(
            input,
            "#[handler] must added to `impl` or `fn`",
        )),
    }
}

fn handle_fn(salvo: &Ident, oapi: &Ident, sig: &Signature) -> syn::Result<(TokenStream, Vec<TokenStream>)> {
    let name = &sig.ident;
    let mut extract_ts = Vec::with_capacity(sig.inputs.len());
    let mut call_args: Vec<Ident> = Vec::with_capacity(sig.inputs.len());
    let mut modifiers = Vec::new();
    for input in &sig.inputs {
        match parse_input_type(input) {
            InputType::Request(_pat) => {
                call_args.push(Ident::new("req", Span::call_site()));
            }
            InputType::Depot(_pat) => {
                call_args.push(Ident::new("depot", Span::call_site()));
            }
            InputType::Response(_pat) => {
                call_args.push(Ident::new("res", Span::call_site()));
            }
            InputType::FlowCtrl(_pat) => {
                call_args.push(Ident::new("ctrl", Span::call_site()));
            }
            InputType::Unknown => {
                return Err(syn::Error::new_spanned(
                    &sig.inputs,
                    "the inputs parameters must be Request, Depot, Response or FlowCtrl",
                ))
            }
            InputType::NoReference(pat) => {
                if let (Pat::Ident(ident), Type::Path(ty)) = (&*pat.pat, &*pat.ty) {
                    call_args.push(ident.ident.clone());
                    // Maybe extractible type.
                    let id = &pat.pat;
                    let ty = omit_type_path_lifetimes(ty);

                    extract_ts.push(quote! {
                        let #id: #ty = match req.extract().await {
                            Ok(data) => data,
                            Err(e) => {
                                #salvo::__private::tracing::error!(error = ?e, "failed to extract data");
                                res.set_status_error(#salvo::http::errors::StatusError::bad_request().with_detail(
                                    "Extract data failed."
                                ));
                                return;
                            }
                        };
                    });
                    modifiers.push(quote! {
                        <#ty as #oapi::endpoint::Modifier<#oapi::oapi::Operation>>::modify(&mut operation);
                    });
                } else {
                    return Err(syn::Error::new_spanned(pat, "Invalid param definition."));
                }
            }
            InputType::Receiver(_) => {
                call_args.push(Ident::new("self", Span::call_site()));
            }
        }
    }

    let hfn = match sig.output {
        ReturnType::Default => {
            if sig.asyncness.is_none() {
                quote! {
                    #[inline]
                    async fn handle(&self, req: &mut #salvo::Request, depot: &mut #salvo::Depot, res: &mut #salvo::Response, ctrl: &mut #salvo::FlowCtrl) {
                        #(#extract_ts)*
                        Self::#name(#(#call_args),*)
                    }
                }
            } else {
                quote! {
                    #[inline]
                    async fn handle(&self, req: &mut #salvo::Request, depot: &mut #salvo::Depot, res: &mut #salvo::Response, ctrl: &mut #salvo::FlowCtrl) {
                        #(#extract_ts)*
                        Self::#name(#(#call_args),*).await
                    }
                }
            }
        }
        ReturnType::Type(_, _) => {
            if sig.asyncness.is_none() {
                quote! {
                    #[inline]
                    async fn handle(&self, req: &mut #salvo::Request, depot: &mut #salvo::Depot, res: &mut #salvo::Response, ctrl: &mut #salvo::FlowCtrl) {
                        #(#extract_ts)*
                        #salvo::Writer::write(Self::#name(#(#call_args),*), req, depot, res).await;
                    }
                }
            } else {
                quote! {
                    #[inline]
                    async fn handle(&self, req: &mut #salvo::Request, depot: &mut #salvo::Depot, res: &mut #salvo::Response, ctrl: &mut #salvo::FlowCtrl) {
                        #(#extract_ts)*
                        #salvo::Writer::write(Self::#name(#(#call_args),*).await, req, depot, res).await;
                    }
                }
            }
        }
    };
    Ok((hfn, modifiers))
}
