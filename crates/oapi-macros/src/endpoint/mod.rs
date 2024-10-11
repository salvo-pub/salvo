use proc_macro2::{Span, TokenStream};
use quote::{quote, ToTokens};
use syn::{Expr, Generics, Ident, ImplItem, Item, Pat, ReturnType, Signature, Type};

use crate::doc_comment::CommentAttributes;
use crate::{omit_type_path_lifetimes, parse_input_type, Array, DiagResult, InputType, Operation};

mod attr;
pub(crate) use attr::EndpointAttr;

fn metadata(
    salvo: &Ident,
    oapi: &Ident,
    attr: EndpointAttr,
    name: &Ident,
    mut modifiers: Vec<TokenStream>,
    generics: &Generics,
) -> DiagResult<TokenStream> {
    let tfn = Ident::new(
        &format!("__macro_gen_oapi_endpoint_type_id_{}", name),
        Span::call_site(),
    );
    let cfn = Ident::new(
        &format!("__macro_gen_oapi_endpoint_creator_{}", name),
        Span::call_site(),
    );
    let opt = Operation::new(&attr);
    modifiers.append(opt.modifiers()?.as_mut());
    let status_codes = Array::from_iter(attr.status_codes.iter().map(|expr| match expr {
        Expr::Lit(lit) => {
            quote! {
                #salvo::http::StatusCode::from_u16(#lit).unwrap()
            }
        }
        _ => {
            quote! {
                #expr
            }
        }
    }));
    let modifiers = if modifiers.is_empty() {
        None
    } else {
        Some(quote! {{
            let mut components = &mut components;
            let mut operation = &mut operation;
            #(#modifiers)*
        }})
    };
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let stream = quote! {
        fn #tfn #impl_generics() -> ::std::any::TypeId #where_clause {
            ::std::any::TypeId::of::<#name #ty_generics>()
        }
        fn #cfn #impl_generics() -> #oapi::oapi::Endpoint #where_clause {
            let mut components = #oapi::oapi::Components::new();
            let status_codes: &[#salvo::http::StatusCode] = &#status_codes;
            let mut operation = #oapi::oapi::Operation::new();
            #modifiers
            if operation.operation_id.is_none() {
                operation.operation_id = Some(#oapi::oapi::naming::assign_name::<#name #ty_generics>(#oapi::oapi::naming::NameRule::Auto));
            }
            if !status_codes.is_empty() {
                let responses = std::ops::DerefMut::deref_mut(&mut operation.responses);
                responses.retain(|k,_| {
                    if let Ok(code) = <#salvo::http::StatusCode as std::str::FromStr>::from_str(k) {
                        status_codes.contains(&code)
                    } else {
                        true
                    }
                });
            }
            #oapi::oapi::Endpoint{
                operation,
                components,
            }
        }
        #oapi::oapi::__private::inventory::submit! {
            #oapi::oapi::EndpointRegistry::save(#tfn, #cfn)
        }
    };
    Ok(stream)
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
            attr.deprecated = if attrs.iter().any(|attr| attr.path().is_ident("deprecated")) {
                Some(true)
            } else {
                None
            };

            let (hfn, modifiers) = handle_fn(&salvo, &oapi, sig)?;
            let meta = metadata(&salvo, &oapi, attr, name, modifiers, &sig.generics)?;
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
            attr.deprecated = if attrs.iter().any(|attr| attr.path().is_ident("deprecated")) {
                Some(true)
            } else {
                None
            };

            let mut hmtd = None;
            for item in &item_impl.items {
                if let ImplItem::Fn(method) = item {
                    if method.sig.ident == Ident::new("handle", Span::call_site()) {
                        hmtd = Some(method);
                    }
                }
            }
            let Some(hmtd) = hmtd else {
                return Err(syn::Error::new_spanned(
                    item_impl.impl_token,
                    "missing handle function",
                ));
            };
            let (hfn, modifiers) = handle_fn(&salvo, &oapi, &hmtd.sig)?;
            let ty = &item_impl.self_ty;
            let (impl_generics, ty_generics, where_clause) = &item_impl.generics.split_for_impl();
            let name = ty
                .to_token_stream()
                .to_string()
                .to_owned()
                .trim_end_matches(&ty_generics.to_token_stream().to_string())
                .trim()
                .to_owned();
            let name = Ident::new(&name, Span::call_site());
            let meta = metadata(&salvo, &oapi, attr, &name, modifiers, &item_impl.generics)?;
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

fn handle_fn(
    salvo: &Ident,
    oapi: &Ident,
    sig: &Signature,
) -> syn::Result<(TokenStream, Vec<TokenStream>)> {
    let name = &sig.ident;
    let mut extract_ts = Vec::with_capacity(sig.inputs.len());
    let mut call_args: Vec<Ident> = Vec::with_capacity(sig.inputs.len());
    let mut modifiers = Vec::new();
    for input in &sig.inputs {
        match parse_input_type(input) {
            InputType::Request(_pat) => {
                call_args.push(Ident::new("__macro_gen_req", Span::call_site()));
            }
            InputType::Depot(_pat) => {
                call_args.push(Ident::new("__macro_gen_depot", Span::call_site()));
            }
            InputType::Response(_pat) => {
                call_args.push(Ident::new("__macro_gen_res", Span::call_site()));
            }
            InputType::FlowCtrl(_pat) => {
                call_args.push(Ident::new("__macro_gen_ctrl", Span::call_site()));
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
                    let ty = omit_type_path_lifetimes(ty);
                    let idv = pat.pat.to_token_stream().to_string();
                    // If id like `mut pdata`, then idv is `pdata`;
                    let idv = idv
                        .rsplit_once(' ')
                        .map(|(_, v)| v.to_owned())
                        .unwrap_or(idv);
                    let id = Ident::new(&idv, Span::call_site());
                    let idv = idv.trim_start_matches('_');
                    extract_ts.push(quote!{
                        let #id: #ty = match <#ty as #salvo::Extractible>::extract_with_arg(__macro_gen_req, #idv).await {
                            Ok(data) => {
                                data
                            },
                            Err(e) => {
                                e.write(__macro_gen_req, __macro_gen_depot, __macro_gen_res).await;
                                // If status code is not set or is not error, set it to 400.
                                let status_code = __macro_gen_res.status_code.unwrap_or_default();
                                if !status_code.is_client_error() && !status_code.is_server_error() {
                                    __macro_gen_res.status_code(#salvo::http::StatusCode::BAD_REQUEST);
                                }
                                return;
                            }
                        };
                    });
                    modifiers.push(quote! {
                         <#ty as #oapi::oapi::EndpointArgRegister>::register(components, operation, #idv);
                    });
                } else {
                    return Err(syn::Error::new_spanned(pat, "invalid param definition"));
                }
            }
            InputType::Receiver(_) => {
                call_args.push(Ident::new("self", Span::call_site()));
            }
        }
    }

    let hfn = match &sig.output {
        ReturnType::Default => {
            if sig.asyncness.is_none() {
                quote! {
                    async fn handle(&self, __macro_gen_req: &mut #salvo::Request, __macro_gen_depot: &mut #salvo::Depot, __macro_gen_res: &mut #salvo::Response, __macro_gen_ctrl: &mut #salvo::FlowCtrl) {
                        #(#extract_ts)*
                        Self::#name(#(#call_args),*)
                    }
                }
            } else {
                quote! {
                    async fn handle(&self, __macro_gen_req: &mut #salvo::Request, __macro_gen_depot: &mut #salvo::Depot, __macro_gen_res: &mut #salvo::Response, __macro_gen_ctrl: &mut #salvo::FlowCtrl) {
                        #(#extract_ts)*
                        Self::#name(#(#call_args),*).await
                    }
                }
            }
        }
        ReturnType::Type(_, ty) => {
            modifiers.push(quote! {
                <#ty as #oapi::oapi::EndpointOutRegister>::register(components, operation);
            });
            if sig.asyncness.is_none() {
                quote! {
                    async fn handle(&self, __macro_gen_req: &mut #salvo::Request, __macro_gen_depot: &mut #salvo::Depot, __macro_gen_res: &mut #salvo::Response, __macro_gen_ctrl: &mut #salvo::FlowCtrl) {
                        #(#extract_ts)*
                        #salvo::Writer::write(Self::#name(#(#call_args),*), __macro_gen_req, __macro_gen_depot, __macro_gen_res).await;
                    }
                }
            } else {
                quote! {
                    async fn handle(&self, __macro_gen_req: &mut #salvo::Request, __macro_gen_depot: &mut #salvo::Depot, __macro_gen_res: &mut #salvo::Response, __macro_gen_ctrl: &mut #salvo::FlowCtrl) {
                        #(#extract_ts)*
                        #salvo::Writer::write(Self::#name(#(#call_args),*).await, __macro_gen_req, __macro_gen_depot, __macro_gen_res).await;
                    }
                }
            }
        }
    };
    Ok((hfn, modifiers))
}
