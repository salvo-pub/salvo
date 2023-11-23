use proc_macro2::{Span, TokenStream};
use quote::{quote, ToTokens};
use syn::{Ident, ImplItem, Item, ReturnType, Signature, Type};

use crate::shared::*;

pub(crate) fn generate(input: Item) -> syn::Result<TokenStream> {
    let salvo = salvo_crate();
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

            let hfn = handle_fn(&salvo, sig)?;
            Ok(quote! {
                #sdef
                #[#salvo::async_trait]
                impl #salvo::Handler for #name {
                    #hfn
                }
            })
        }
        Item::Impl(item_impl) => {
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
            let hfn = handle_fn(&salvo, &hmtd.sig)?;
            let ty = &item_impl.self_ty;
            let (impl_generics, _, where_clause) = &item_impl.generics.split_for_impl();

            Ok(quote! {
                #item_impl
                #[#salvo::async_trait]
                impl #impl_generics #salvo::Handler for #ty #where_clause {
                    #hfn
                }
            })
        }
        _ => Err(syn::Error::new_spanned(
            input,
            "#[handler] must added to `impl` or `fn`",
        )),
    }
}

fn handle_fn(salvo: &Ident, sig: &Signature) -> syn::Result<TokenStream> {
    let name = &sig.ident;
    let mut extract_ts = Vec::with_capacity(sig.inputs.len());
    let mut call_args: Vec<Ident> = Vec::with_capacity(sig.inputs.len());
    let mut count = 0;
    for input in &sig.inputs {
        match parse_input_type(input) {
            InputType::Request(_pat) => {
                call_args.push(Ident::new("__macro_generated_req", Span::call_site()));
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
                if let (_, Type::Path(ty)) = (&*pat.pat, &*pat.ty) {
                    let id = Ident::new(&format!("s{count}"), Span::call_site());
                    let ty = omit_type_path_lifetimes(ty);
                    let idv = id.to_token_stream().to_string();

                    extract_ts.push(quote! {
                        let #id: #ty = match <#ty as #salvo::Extractible>::extract_with_arg(__macro_generated_req, #idv).await {
                            Ok(data) => data,
                            Err(e) => {
                                #salvo::__private::tracing::error!(error = ?e, "failed to extract data");
                                res.render(#salvo::http::errors::StatusError::bad_request().brief(
                                    "Extract data failed."
                                ).cause(e));
                                return;
                            }
                        };
                    });
                    call_args.push(id);
                    count += 1;
                } else {
                    return Err(syn::Error::new_spanned(pat, "invalid param definition"));
                }
            }
            InputType::Receiver(_) => {
                call_args.push(Ident::new("self", Span::call_site()));
            }
        }
    }

    match sig.output {
        ReturnType::Default => {
            if sig.asyncness.is_none() {
                Ok(quote! {
                    #[inline]
                    async fn handle(&self, __macro_generated_req: &mut #salvo::Request, depot: &mut #salvo::Depot, res: &mut #salvo::Response, ctrl: &mut #salvo::FlowCtrl) {
                        #(#extract_ts)*
                        Self::#name(#(#call_args),*)
                    }
                })
            } else {
                Ok(quote! {
                    #[inline]
                    async fn handle(&self, __macro_generated_req: &mut #salvo::Request, depot: &mut #salvo::Depot, res: &mut #salvo::Response, ctrl: &mut #salvo::FlowCtrl) {
                        #(#extract_ts)*
                        Self::#name(#(#call_args),*).await
                    }
                })
            }
        }
        ReturnType::Type(_, _) => {
            if sig.asyncness.is_none() {
                Ok(quote! {
                    #[inline]
                    async fn handle(&self, __macro_generated_req: &mut #salvo::Request, depot: &mut #salvo::Depot, res: &mut #salvo::Response, ctrl: &mut #salvo::FlowCtrl) {
                        #(#extract_ts)*
                        #salvo::Writer::write(Self::#name(#(#call_args),*), __macro_generated_req, depot, res).await;
                    }
                })
            } else {
                Ok(quote! {
                    #[inline]
                    async fn handle(&self, __macro_generated_req: &mut #salvo::Request, depot: &mut #salvo::Depot, res: &mut #salvo::Response, ctrl: &mut #salvo::FlowCtrl) {
                        #(#extract_ts)*
                        #salvo::Writer::write(Self::#name(#(#call_args),*).await, __macro_generated_req, depot, res).await;
                    }
                })
            }
        }
    }
}
