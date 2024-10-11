//! This is **private** salvo_oapi codegen library and is not used alone.
//!
//! The library contains macro implementations for salvo_oapi library. Content
//! of the library documentation is available through **salvo_oapi** library itself.
//! Consider browsing via the **salvo_oapi** crate so all links will work correctly.

#![doc(html_favicon_url = "https://salvo.rs/favicon-32x32.png")]
#![doc(html_logo_url = "https://salvo.rs/images/logo.svg")]
#![cfg_attr(docsrs, feature(doc_cfg))]

use proc_macro::TokenStream;
use quote::ToTokens;
use syn::parse::{Parse, ParseStream};
use syn::token::Bracket;
use syn::{bracketed, parse_macro_input, Ident, Item, Token};

#[macro_use]
mod cfg;
mod attribute;
pub(crate) mod bound;
mod component;
mod doc_comment;
mod endpoint;
pub(crate) mod feature;
mod operation;
mod parameter;
pub(crate) mod parse_utils;
mod response;
mod schema;
mod schema_type;
mod security_requirement;
mod server;
mod shared;
mod type_tree;

pub(crate) use self::{
    component::{ComponentSchema, ComponentSchemaProps},
    endpoint::EndpointAttr,
    feature::Feature,
    operation::Operation,
    parameter::Parameter,
    response::Response,
    server::Server,
    shared::*,
    type_tree::TypeTree,
};
pub(crate) use proc_macro2_diagnostics::{Diagnostic, Level as DiagLevel};
pub(crate) use salvo_serde_util::{self as serde_util, RenameRule, SerdeContainer, SerdeValue};

/// Enhanced of [handler][handler] for generate OpenAPI documention, [Read more][more].
///
/// [handler]: ../salvo_core/attr.handler.html
/// [more]: ../salvo_oapi/endpoint/index.html
#[proc_macro_attribute]
pub fn endpoint(attr: TokenStream, input: TokenStream) -> TokenStream {
    let attr = syn::parse_macro_input!(attr as EndpointAttr);
    let item = parse_macro_input!(input as Item);
    match endpoint::generate(attr, item) {
        Ok(stream) => {
            // Temporary debug printing.
            println!("{}", stream.to_token_stream());

            stream.into()
        }
        Err(e) => e.to_compile_error().into(),
    }
}
/// This is `#[derive]` implementation for [`ToSchema`][to_schema] trait, [Read more][more].
///
/// [to_schema]: ../salvo_oapi/trait.ToSchema.html
/// [more]: ../salvo_oapi/derive.ToSchema.html
#[proc_macro_derive(ToSchema, attributes(salvo))] //attributes(schema)
pub fn derive_to_schema(input: TokenStream) -> TokenStream {
    match schema::to_schema(syn::parse_macro_input!(input)) {
        Ok(stream) => stream.into(),
        Err(e) => e.emit_as_item_tokens().into(),
    }
}

/// Generate parameters from struct's fields, [Read more][more].
///
/// [more]: ../salvo_oapi/derive.ToParameters.html
#[proc_macro_derive(ToParameters, attributes(salvo))] //attributes(parameter, parameters)
pub fn derive_to_parameters(input: TokenStream) -> TokenStream {
    match parameter::to_parameters(syn::parse_macro_input!(input)) {
        Ok(stream) => stream.into(),
        Err(e) => e.emit_as_item_tokens().into(),
    }
}

/// Generate reusable [OpenApi][openapi] response, [Read more][more].
///
/// [openapi]: ../salvo_oapi/struct.OpenApi.html
/// [more]: ../salvo_oapi/derive.ToResponse.html
#[proc_macro_derive(ToResponse, attributes(salvo))] //attributes(response, content, schema))
pub fn derive_to_response(input: TokenStream) -> TokenStream {
    match response::to_response(syn::parse_macro_input!(input)) {
        Ok(stream) => stream.into(),
        Err(e) => e.emit_as_item_tokens().into(),
    }
}

/// Generate responses with status codes what can be used in [OpenAPI][openapi], [Read more][more].
///
/// [openapi]: ../salvo_oapi/struct.OpenApi.html
/// [more]: ../salvo_oapi/derive.ToResponses.html
#[proc_macro_derive(ToResponses, attributes(salvo))] //attributes(response, schema, ref_response, response))
pub fn to_responses(input: TokenStream) -> TokenStream {
    match response::to_responses(syn::parse_macro_input!(input)) {
        Ok(stream) => stream.into(),
        Err(e) => e.emit_as_item_tokens().into(),
    }
}

#[doc(hidden)]
#[proc_macro]
pub fn schema(input: TokenStream) -> TokenStream {
    struct Schema {
        inline: bool,
        ty: syn::Type,
    }
    impl Parse for Schema {
        fn parse(input: ParseStream) -> syn::Result<Self> {
            let inline = if input.peek(Token![#]) && input.peek2(Bracket) {
                input.parse::<Token![#]>()?;

                let inline;
                bracketed!(inline in input);
                let i = inline.parse::<Ident>()?;
                i == "inline"
            } else {
                false
            };

            let ty = input.parse()?;
            Ok(Self { inline, ty })
        }
    }

    let schema = syn::parse_macro_input!(input as Schema);
    let type_tree = match TypeTree::from_type(&schema.ty) {
        Ok(type_tree) => type_tree,
        Err(diag) => return diag.emit_as_item_tokens().into(),
    };

    let stream = ComponentSchema::new(ComponentSchemaProps {
        features: Some(vec![Feature::Inline(schema.inline.into())]),
        type_tree: &type_tree,
        deprecated: None,
        description: None,
        object_name: "",
    })
    .map(|s| s.to_token_stream());
    match stream {
        Ok(stream) => stream.into(),
        Err(diag) => diag.emit_as_item_tokens().into(),
    }
}

pub(crate) trait IntoInner<T> {
    fn into_inner(self) -> T;
}

#[cfg(test)]
mod tests {
    use quote::quote;
    use syn::parse2;

    use super::*;

    #[test]
    fn test_endpoint_for_fn() {
        let input = quote! {
            #[endpoint]
            async fn hello() {
                res.render_plain_text("Hello World");
            }
        };
        let item = parse2(input).unwrap();
        assert_eq!(
            endpoint::generate(parse2(quote! {}).unwrap(), item)
                .unwrap()
                .to_string(),
            quote! {
                #[allow(non_camel_case_types)]
                #[derive(Debug)]
                struct hello;
                impl hello {
                    #[endpoint]
                    async fn hello() {
                        {res.render_plain_text("Hello World");}
                    }
                }
                #[salvo::async_trait]
                impl salvo::Handler for hello {
                    async fn handle(
                        &self,
                        __macro_gen_req: &mut salvo::Request,
                        __macro_gen_depot: &mut salvo::Depot,
                        __macro_gen_res: &mut salvo::Response,
                        __macro_gen_ctrl: &mut salvo::FlowCtrl
                    ) {
                        Self::hello().await
                    }
                }
                fn __macro_gen_oapi_endpoint_type_id_hello() -> ::std::any::TypeId {
                    ::std::any::TypeId::of::<hello>()
                }
                fn __macro_gen_oapi_endpoint_creator_hello() -> salvo::oapi::Endpoint {
                    let mut components = salvo::oapi::Components::new();
                    let status_codes: &[salvo::http::StatusCode] = &[];
                    let mut operation = salvo::oapi::Operation::new();
                    if operation.operation_id.is_none() {
                        operation.operation_id = Some(salvo::oapi::naming::assign_name::<hello>(salvo::oapi::naming::NameRule::Auto));
                    }
                    if !status_codes.is_empty() {
                        let responses = std::ops::DerefMut::deref_mut(&mut operation.responses);
                        responses.retain(|k, _| {
                            if let Ok(code) = <salvo::http::StatusCode as std::str::FromStr>::from_str(k) {
                                status_codes.contains(&code)
                            } else {
                                true
                            }
                        });
                    }
                    salvo::oapi::Endpoint {
                        operation,
                        components,
                    }
                }
                salvo::oapi::__private::inventory::submit! {
                    salvo::oapi::EndpointRegistry::save(__macro_gen_oapi_endpoint_type_id_hello, __macro_gen_oapi_endpoint_creator_hello)
                }
            }
            .to_string()
        );
    }

    #[test]
    fn test_to_schema_struct() {
        let input = quote! {
            /// This is user.
            ///
            /// This is user description.
            #[derive(ToSchema)]
            struct User {
                #[salvo(schema(examples("chris"), min_length = 1, max_length = 100, required))]
                name: String,
                #[salvo(schema(example = 16, default = 0, maximum=100, minimum=0,format = "int32"))]
                age: i32,
                #[deprecated = "There is deprecated"]
                high: u32,
            }
        };
        assert_eq!(
            schema::to_schema(parse2(input).unwrap()).unwrap()
                .to_string(),
            quote! {
                impl salvo::oapi::ToSchema for User {
                    fn to_schema(components: &mut salvo::oapi::Components) -> salvo::oapi::RefOr<salvo::oapi::schema::Schema> {
                        let name = salvo::oapi::naming::assign_name::<User>(salvo::oapi::naming::NameRule::Auto);
                        let ref_or = salvo::oapi::RefOr::Ref(salvo::oapi::Ref::new(format!("#/components/schemas/{}", name)));
                        if !components.schemas.contains_key(&name) {
                            components.schemas.insert(name.clone(), ref_or.clone());
                            let schema = salvo::oapi::Object::new()
                                .property(
                                    "name",
                                    salvo::oapi::Object::new()
                                        .schema_type(salvo::oapi::schema::SchemaType::basic(salvo::oapi::schema::BasicType::String))
                                        .examples([salvo::oapi::__private::serde_json::json!("chris"),])
                                        .min_length(1usize)
                                        .max_length(100usize)
                                )
                                .required("name")
                                .property(
                                    "age",
                                    salvo::oapi::Object::new()
                                        .schema_type(salvo::oapi::schema::SchemaType::basic(salvo::oapi::schema::BasicType::Integer))
                                        .format(salvo::oapi::SchemaFormat::KnownFormat(salvo::oapi::KnownFormat::Int32))
                                        .example(salvo::oapi::__private::serde_json::json!(16))
                                        .default_value(salvo::oapi::__private::serde_json::json!(0))
                                        .maximum(100f64)
                                        .minimum(0f64)
                                        .format(salvo::oapi::SchemaFormat::Custom(String::from("int32")))
                                )
                                .required("age")
                                .property(
                                    "high",
                                    salvo::oapi::Object::new()
                                        .schema_type(salvo::oapi::schema::SchemaType::basic(salvo::oapi::schema::BasicType::Integer))
                                        .format(salvo::oapi::SchemaFormat::KnownFormat(salvo::oapi::KnownFormat::UInt32))
                                        .deprecated(salvo::oapi::Deprecated::True)
                                        .minimum(0f64)
                                )
                                .required("high")
                                .description("This is user.\n\nThis is user description.");
                            components.schemas.insert(name, schema);
                        }
                        ref_or
                    }
                }
            } .to_string()
        );
    }

    #[test]
    fn test_to_schema_generics() {
        let input = quote! {
            #[derive(Serialize, Deserialize, ToSchema, Debug)]
            #[salvo(schema(aliases(MyI32 = MyObject<i32>, MyStr = MyObject<String>)))]
            struct MyObject<T: ToSchema + std::fmt::Debug + 'static> {
                value: T,
            }
        };
        assert_eq!(
            schema::to_schema(parse2(input).unwrap()).unwrap()
                .to_string().replace("< ", "<").replace("> ", ">"),
            quote! {
                impl<T: ToSchema + std::fmt::Debug + 'static> salvo::oapi::ToSchema for MyObject<T>
                where
                    T: salvo::oapi::ToSchema + 'static
                {
                    fn to_schema(components: &mut salvo::oapi::Components) -> salvo::oapi::RefOr<salvo::oapi::schema::Schema> {
                        let mut name = None;
                        if ::std::any::TypeId::of::<Self>() == ::std::any::TypeId::of::<MyObject<i32>>() {
                            name = Some(salvo::oapi::naming::assign_name::<MyObject<i32>>(
                                salvo::oapi::naming::NameRule::Force("MyI32")
                            ));
                        }
                        if ::std::any::TypeId::of::<Self>() == ::std::any::TypeId::of::<MyObject<String>>() {
                            name = Some(salvo::oapi::naming::assign_name::<MyObject<String>>(
                                salvo::oapi::naming::NameRule::Force("MyStr")
                            ));
                        }
                        let name = name
                            .unwrap_or_else(|| salvo::oapi::naming::assign_name::<MyObject<T>>(salvo::oapi::naming::NameRule::Auto));
                        let ref_or = salvo::oapi::RefOr::Ref(salvo::oapi::Ref::new(format!("#/components/schemas/{}", name)));
                        if !components.schemas.contains_key(&name) {
                            components.schemas.insert(name.clone(), ref_or.clone());
                            let schema = salvo::oapi::Object::new()
                                .property(
                                    "value",
                                    salvo::oapi::RefOr::from(<T as salvo::oapi::ToSchema>::to_schema(components))
                                )
                                .required("value");
                            components.schemas.insert(name, schema);
                        }
                        ref_or
                    }
                }
            } .to_string().replace("< ", "<").replace("> ", ">")
        );
    }

    #[test]
    fn test_to_schema_enum() {
        let input = quote! {
            #[derive(Serialize, Deserialize, ToSchema, Debug)]
            #[salvo(schema(rename_all = "camelCase"))]
            enum People {
                Man,
                Woman,
            }
        };
        assert_eq!(
            schema::to_schema(parse2(input).unwrap()).unwrap()
                .to_string(),
            quote! {
                impl salvo::oapi::ToSchema for People {
                    fn to_schema(components: &mut salvo::oapi::Components) -> salvo::oapi::RefOr<salvo::oapi::schema::Schema> {
                        let name = salvo::oapi::naming::assign_name::<People>(salvo::oapi::naming::NameRule::Auto);
                        let ref_or = salvo::oapi::RefOr::Ref(salvo::oapi::Ref::new(format!("#/components/schemas/{}", name)));
                        if !components.schemas.contains_key(&name) {
                            components.schemas.insert(name.clone(), ref_or.clone());
                            let schema = salvo::oapi::Object::new()
                                .schema_type(salvo::oapi::schema::SchemaType::basic(salvo::oapi::schema::BasicType::String))
                                .enum_values::<[&str; 2usize], &str>(["man", "woman",]);
                            components.schemas.insert(name, schema);
                        }
                        ref_or
                    }
                }
            } .to_string()
        );
    }

    #[test]
    fn test_to_response() {
        let input = quote! {
            #[derive(ToResponse)]
            #[salvo(response(description = "Person response returns single Person entity"))]
            struct User{
                name: String,
                age: i32,
            }
        };
        assert_eq!(
            response::to_response(parse2(input).unwrap()).unwrap()
                .to_string(),
            quote! {
                impl salvo::oapi::ToResponse for User {
                    fn to_response(
                        components: &mut salvo::oapi::Components
                    ) -> salvo::oapi::RefOr<salvo::oapi::Response> {
                        let response = salvo::oapi::Response::new("Person response returns single Person entity").add_content(
                            "application/json",
                            salvo::oapi::Content::new(
                                salvo::oapi::Object::new()
                                    .property(
                                        "name",
                                        salvo::oapi::Object::new().schema_type(salvo::oapi::schema::SchemaType::basic(salvo::oapi::schema::BasicType::String))
                                    )
                                    .required("name")
                                    .property(
                                        "age",
                                        salvo::oapi::Object::new()
                                            .schema_type(salvo::oapi::schema::SchemaType::basic(salvo::oapi::schema::BasicType::Integer))
                                            .format(salvo::oapi::SchemaFormat::KnownFormat(
                                                salvo::oapi::KnownFormat::Int32
                                            ))
                                    )
                                    .required("age")
                            )
                        );
                        components.responses.insert("User", response);
                        salvo::oapi::RefOr::Ref(salvo::oapi::Ref::new(format!("#/components/responses/{}", "User")))
                    }
                }
                impl salvo::oapi::EndpointOutRegister for User {
                    fn register(components: &mut salvo::oapi::Components, operation: &mut salvo::oapi::Operation) {
                        operation
                            .responses
                            .insert("200", <Self as salvo::oapi::ToResponse>::to_response(components))
                    }
                }
            } .to_string()
        );
    }

    #[test]
    fn test_to_responses() {
        let input = quote! {
            #[derive(salvo_oapi::ToResponses)]
            enum UserResponses {
                /// Success response description.
                #[salvo(response(status_code = 200))]
                Success { value: String },

                #[salvo(response(status_code = 404))]
                NotFound,

                #[salvo(response(status_code = 400))]
                BadRequest(BadRequest),

                #[salvo(response(status_code = 500))]
                ServerError(Response),

                #[salvo(response(status_code = 418))]
                TeaPot(Response),
            }
        };
        assert_eq!(
            response::to_responses(parse2(input).unwrap()).unwrap().to_string(),
            quote! {
                impl salvo::oapi::ToResponses for UserResponses {
                    fn to_responses(components: &mut salvo::oapi::Components) -> salvo::oapi::response::Responses {
                        [
                            (
                                "200",
                                salvo::oapi::RefOr::from(
                                    salvo::oapi::Response::new("Success response description.").add_content(
                                        "application/json",
                                        salvo::oapi::Content::new(
                                            salvo::oapi::Object::new()
                                                .property(
                                                    "value",
                                                    salvo::oapi::Object::new().schema_type(salvo::oapi::schema::SchemaType::basic(salvo::oapi::schema::BasicType::String))
                                                )
                                                .required("value")
                                                .description("Success response description.")
                                        )
                                    )
                                )
                            ),
                            (
                                "404",
                                salvo::oapi::RefOr::from(salvo::oapi::Response::new(""))
                            ),
                            (
                                "400",
                                salvo::oapi::RefOr::from(salvo::oapi::Response::new("").add_content(
                                    "application/json",
                                    salvo::oapi::Content::new(salvo::oapi::RefOr::from(
                                        <BadRequest as salvo::oapi::ToSchema>::to_schema(components)
                                    ))
                                ))
                            ),
                            (
                                "500",
                                salvo::oapi::RefOr::from(salvo::oapi::Response::new("").add_content(
                                    "application/json",
                                    salvo::oapi::Content::new(salvo::oapi::RefOr::from(
                                        <Response as salvo::oapi::ToSchema>::to_schema(components)
                                    ))
                                ))
                            ),
                            (
                                "418",
                                salvo::oapi::RefOr::from(salvo::oapi::Response::new("").add_content(
                                    "application/json",
                                    salvo::oapi::Content::new(salvo::oapi::RefOr::from(
                                        <Response as salvo::oapi::ToSchema>::to_schema(components)
                                    ))
                                ))
                            ),
                        ]
                        .into()
                    }
                }
                impl salvo::oapi::EndpointOutRegister for UserResponses {
                    fn register(components: &mut salvo::oapi::Components, operation: &mut salvo::oapi::Operation) {
                        operation
                            .responses
                            .append(&mut <Self as salvo::oapi::ToResponses>::to_responses(components));
                    }
                }
            }
            .to_string()
        );
    }

    #[test]
    fn test_to_parameters() {
        let input = quote! {
            #[derive(Deserialize, ToParameters)]
            struct PetQuery {
                /// Name of pet
                name: Option<String>,
                /// Age of pet
                age: Option<i32>,
                /// Kind of pet
                #[salvo(parameter(inline))]
                kind: PetKind
            }
        };
        assert_eq!(
            parameter::to_parameters(parse2(input).unwrap()).unwrap().to_string(),
            quote! {
                impl<'__macro_gen_ex> salvo::oapi::ToParameters<'__macro_gen_ex> for PetQuery {
                    fn to_parameters(components: &mut salvo::oapi::Components) -> salvo::oapi::Parameters {
                        salvo::oapi::Parameters(
                            [
                                salvo::oapi::parameter::Parameter::new("name")
                                    .description("Name of pet")
                                    .required(salvo::oapi::Required::False)
                                    .schema(
                                        salvo::oapi::Object::new()
                                            .schema_type(salvo::oapi::schema::SchemaType::from_iter([salvo::oapi::schema::BasicType::String, salvo::oapi::schema::BasicType::Null]))
                                    ),
                                salvo::oapi::parameter::Parameter::new("age")
                                    .description("Age of pet")
                                    .required(salvo::oapi::Required::False)
                                    .schema(
                                        salvo::oapi::Object::new()
                                            .schema_type(salvo::oapi::schema::SchemaType::from_iter([salvo::oapi::schema::BasicType::Integer, salvo::oapi::schema::BasicType::Null]))
                                            .format(salvo::oapi::SchemaFormat::KnownFormat(
                                                salvo::oapi::KnownFormat::Int32
                                            ))
                                    ),
                                salvo::oapi::parameter::Parameter::new("kind")
                                    .description("Kind of pet")
                                    .required(salvo::oapi::Required::True)
                                    .schema(<PetKind as salvo::oapi::ToSchema>::to_schema(components)),
                            ]
                            .to_vec()
                        )
                    }
                }
                impl salvo::oapi::EndpointArgRegister for PetQuery {
                    fn register(
                        components: &mut salvo::oapi::Components,
                        operation: &mut salvo::oapi::Operation,
                        _arg: &str
                    ) {
                        for parameter in <Self as salvo::oapi::ToParameters>::to_parameters(components) {
                            operation.parameters.insert(parameter);
                        }
                    }
                }
                impl<'__macro_gen_ex> salvo::Extractible<'__macro_gen_ex> for PetQuery {
                    fn metadata() -> &'__macro_gen_ex salvo::extract::Metadata {
                        static METADATA: ::std::sync::OnceLock<salvo::extract::Metadata> = ::std::sync::OnceLock::new();
                        METADATA.get_or_init(||
                            salvo::extract::Metadata::new("PetQuery")
                                .default_sources(vec![salvo::extract::metadata::Source::new(
                                    salvo::extract::metadata::SourceFrom::Query,
                                    salvo::extract::metadata::SourceParser::MultiMap
                                )])
                                .fields(vec![
                                    salvo::extract::metadata::Field::new("name"),
                                    salvo::extract::metadata::Field::new("age"),
                                    salvo::extract::metadata::Field::new("kind")
                                ])
                        )
                    }
                    async fn extract(
                        req: &'__macro_gen_ex mut salvo::Request
                    ) -> Result<Self, impl salvo::Writer + Send + std::fmt::Debug + 'static> {
                        salvo::serde::from_request(req, Self::metadata()).await
                    }
                    async fn extract_with_arg(
                        req: &'__macro_gen_ex mut salvo::Request,
                        _arg: &str
                    ) -> Result<Self, impl salvo::Writer + Send + std::fmt::Debug + 'static> {
                        Self::extract(req).await
                    }
                }
            }
            .to_string()
        );
    }
}
