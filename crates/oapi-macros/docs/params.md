Generate [path parameters][path_params] from struct's
fields.

This is `#[derive]` implementation for [`IntoParams`][into_params] trait.

Typically path parameters need to be defined within [`#[salvo_oapi::path(...params(...))]`][path_params] section
for the endpoint. But this trait eliminates the need for that when [`struct`][struct]s are used to define parameters.
Still [`std::primitive`] and [`String`](std::string::String) path parameters or [`tuple`] style path parameters need to be defined
within `params(...)` section if description or other than default configuration need to be given.

You can use the Rust's own `#[deprecated]` attribute on field to mark it as
deprecated and it will reflect to the generated OpenAPI spec.

`#[deprecated]` attribute supports adding additional details such as a reason and or since version
but this is is not supported in OpenAPI. OpenAPI has only a boolean flag to determine deprecation.
While it is totally okay to declare deprecated with reason
`#[deprecated  = "There is better way to do this"]` the reason would not render in OpenAPI spec.

Doc comment on struct fields will be used as description for the generated parameters.
```
#[derive(salvo_oapi::IntoParams)]
struct Query {
    /// Query todo items by name.
    name: String
}
```

# IntoParams Container Attributes for `#[into_params(...)]`

The following attributes are available for use in on the container attribute `#[into_params(...)]` for the struct
deriving `IntoParams`:

* `names(...)` Define comma separated list of names for unnamed fields of struct used as a path parameter.
   __Only__ supported on __unnamed structs__.
* `style = ...` Defines how all parameters are serialized by [`ParameterStyle`][style]. Default
   values are based on _`parameter_in`_ attribute.
* `parameter_in = ...` =  Defines where the parameters of this field are used with a value from
   [`openapi::path::ParameterIn`][in_enum]. There is no default value, if this attribute is not
   supplied, then the value is determined by the `parameter_in_provider` in
   [`IntoParams::into_params()`](trait.IntoParams.html#tymethod.into_params).
* `rename_all = ...` Can be provided to alternatively to the serde's `rename_all` attribute. Effectively provides same functionality.

Use `names` to define name for single unnamed argument.
```
# use salvo_oapi::IntoParams;
#
#[derive(IntoParams)]
#[into_params(names("id"))]
struct Id(u64);
```

Use `names` to define names for multiple unnamed arguments.
```
# use salvo_oapi::IntoParams;
#
#[derive(IntoParams)]
#[into_params(names("id", "name"))]
struct IdAndName(u64, String);
```

# IntoParams Field Attributes for `#[param(...)]`

The following attributes are available for use in the `#[param(...)]` on struct fields:

* `style = ...` Defines how the parameter is serialized by [`ParameterStyle`][style]. Default values are based on _`parameter_in`_ attribute.

* `explode` Defines whether new _`parameter=value`_ pair is created for each parameter withing _`object`_ or _`array`_.

* `allow_reserved` Defines whether reserved characters _`:/?#[]@!$&'()*+,;=`_ is allowed within value.

* `example = ...` Can be method reference or _`json!(...)`_. Given example
  will override any example in underlying parameter type.

* `value_type = ...` Can be used to override default type derived from type of the field used in OpenAPI spec.
  This is useful in cases where the default type does not correspond to the actual type e.g. when
  any third-party types are used which are not [`ToSchema`][to_schema]s nor [`primitive` types][primitive].
   Value can be any Rust type what normally could be used to serialize to JSON or custom type such as _`Object`_.
   _`Object`_ will be rendered as generic OpenAPI object.

* `inline` If set, the schema for this field's type needs to be a [`ToSchema`][to_schema], and
  the schema definition will be inlined.

* `default = ...` Can be method reference or _`json!(...)`_.

* `format = ...` May either be variant of the [`KnownFormat`][known_format] enum, or otherwise
  an open value as a string. By default the format is derived from the type of the property
  according OpenApi spec.

* `write_only` Defines property is only used in **write** operations *POST,PUT,PATCH* but not in *GET*

* `read_only` Defines property is only used in **read** operations *GET* but not in *POST,PUT,PATCH*

* `xml(...)` Can be used to define [`Xml`][xml] object properties applicable to named fields.
   See configuration options at xml attributes of [`ToSchema`][to_schema_xml]

* `nullable` Defines property is nullable (note this is different to non-required).

* `required = ...` Can be used to enforce required status for the parameter. [See
   rules][derive@IntoParams#field-nullability-and-required-rules]

* `rename = ...` Can be provided to alternatively to the serde's `rename` attribute. Effectively provides same functionality.

* `multiple_of = ...` Can be used to define multiplier for a value. Value is considered valid
  division will result an `integer`. Value must be strictly above _`0`_.

* `maximum = ...` Can be used to define inclusive upper bound to a `number` value.

* `minimum = ...` Can be used to define inclusive lower bound to a `number` value.

* `exclusive_maximum = ...` Can be used to define exclusive upper bound to a `number` value.

* `exclusive_minimum = ...` Can be used to define exclusive lower bound to a `number` value.

* `max_length = ...` Can be used to define maximum length for `string` types.

* `min_length = ...` Can be used to define minimum length for `string` types.

* `pattern = ...` Can be used to define valid regular expression in _ECMA-262_ dialect the field value must match.

* `max_items = ...` Can be used to define maximum items allowed for `array` fields. Value must
  be non-negative integer.

* `min_items = ...` Can be used to define minimum items allowed for `array` fields. Value must
  be non-negative integer.

* `with_schema = ...` Use _`schema`_ created by provided function reference instead of the
  default derived _`schema`_. The function must match to `fn() -> Into<RefOr<Schema>>`. It does
  not accept arguments and must return anything that can be converted into `RefOr<Schema>`.

* `additional_properties = ...` Can be used to define free form types for maps such as
  [`HashMap`](std::collections::HashMap) and [`BTreeMap`](std::collections::BTreeMap).
  Free form type enables use of arbitrary types within map values.
  Supports formats _`additional_properties`_ and _`additional_properties = true`_.

#### Field nullability and required rules

Same rules for nullability and required status apply for _`IntoParams`_ field attributes as for
_`ToSchema`_ field attributes. [See the rules][`derive@ToSchema#field-nullability-and-required-rules`].

# Partial `#[serde(...)]` attributes support

IntoParams derive has partial support for [serde attributes]. These supported attributes will reflect to the
generated OpenAPI doc. The following attributes are currently supported:

* `rename_all = "..."` Supported at the container level.
* `rename = "..."` Supported **only** at the field level.
* `default` Supported at the container level and field level according to [serde attributes].
* `skip_serializing_if = "..."` Supported  **only** at the field level.
* `with = ...` Supported **only** at field level.

Other _`serde`_ attributes will impact the serialization but will not be reflected on the generated OpenAPI doc.

# Examples

_**Demonstrate [`IntoParams`][into_params] usage with resolving `Path` and `Query` parameters
with _`salvo`_**_.
```
use serde::Deserialize;
use serde_json::json;
use salvo_oapi::IntoParams;

#[derive(Deserialize, IntoParams)]
struct PetPathArgs {
    /// Id of pet
    id: i64,
    /// Name of pet
    name: String,
}

#[derive(Deserialize, IntoParams)]
struct Filter {
    /// Age filter for pets
    #[deprecated]
    #[param(style = Form, explode, allow_reserved, example = json!([10]))]
    age: Option<Vec<i32>>,
}

#[salvo_oapi::path(
    params(PetPathArgs, Filter),
    responses(
        (status = 200, description = "success response")
    )
)]
#[get("/pet/{id}/{name}")]
async fn get_pet(pet: Path<PetPathArgs>, query: Query<Filter>) -> impl Responder {
    HttpResponse::Ok().json(json!({ "id": pet.id }))
}
```

_**Demonstrate [`IntoParams`][into_params] usage with the `#[into_params(...)]` container attribute to
be used as a path query, and inlining a schema query field:**_
```
use serde::Deserialize;
use salvo_oapi::{IntoParams, ToSchema};

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
enum PetKind {
    Dog,
    Cat,
}

#[derive(Deserialize, IntoParams)]
#[into_params(style = Form, parameter_in = Query)]
struct PetQuery {
    /// Name of pet
    name: Option<String>,
    /// Age of pet
    age: Option<i32>,
    /// Kind of pet
    #[param(inline)]
    kind: PetKind
}

#[salvo_oapi::path(
    get,
    path = "/get_pet",
    params(PetQuery),
    responses(
        (status = 200, description = "success response")
    )
)]
async fn get_pet(query: PetQuery) {
    // ...
}
```

_**Override `String` with `i64` using `value_type` attribute.**_
```
# use salvo_oapi::IntoParams;
#
#[derive(IntoParams)]
#[into_params(parameter_in = Query)]
struct Filter {
    #[param(value_type = i64)]
    id: String,
}
```

_**Override `String` with `Object` using `value_type` attribute. _`Object`_ will render as `type: object` in OpenAPI spec.**_
```
# use salvo_oapi::IntoParams;
#
#[derive(IntoParams)]
#[into_params(parameter_in = Query)]
struct Filter {
    #[param(value_type = Object)]
    id: String,
}
```

_**You can use a generic type to override the default type of the field.**_
```
# use salvo_oapi::IntoParams;
#
#[derive(IntoParams)]
#[into_params(parameter_in = Query)]
struct Filter {
    #[param(value_type = Option<String>)]
    id: String
}
```

_**You can even override a [`Vec`] with another one.**_
```
# use salvo_oapi::IntoParams;
#
#[derive(IntoParams)]
#[into_params(parameter_in = Query)]
struct Filter {
    #[param(value_type = Vec<i32>)]
    id: Vec<String>
}
```

_**We can override value with another [`ToSchema`][to_schema].**_
```
# use salvo_oapi::{IntoParams, ToSchema};
#
#[derive(ToSchema)]
struct Id {
    value: i64,
}

#[derive(IntoParams)]
#[into_params(parameter_in = Query)]
struct Filter {
    #[param(value_type = Id)]
    id: String
}
```

_**Example with validation attributes.**_
```
#[derive(salvo_oapi::IntoParams)]
struct Item {
    #[param(maximum = 10, minimum = 5, multiple_of = 2.5)]
    id: i32,
    #[param(max_length = 10, min_length = 5, pattern = "[a-z]*")]
    value: String,
    #[param(max_items = 5, min_items = 1)]
    items: Vec<String>,
}
````

_**Use `schema_with` to manually implement schema for a field.**_
```
# use salvo_oapi::openapi::schema::Object;
fn custom_type() -> Object {
    Object::new()
        .schema_type(salvo_oapi::openapi::SchemaType::String)
        .format(Some(salvo_oapi::openapi::SchemaFormat::Custom(
            "email".to_string(),
        )))
        .description(Some("this is the description"))
        .build()
}

#[derive(salvo_oapi::IntoParams)]
#[into_params(parameter_in = Query)]
struct Query {
    #[param(schema_with = custom_type)]
    email: String,
}
```

[to_schema]: trait.ToSchema.html
[known_format]: openapi/schema/enum.KnownFormat.html
[xml]: openapi/xml/struct.Xml.html
[into_params]: trait.IntoParams.html
[path_params]: attr.path.html#params-attributes
[struct]: https://doc.rust-lang.org/std/keyword.struct.html
[style]: openapi/path/enum.ParameterStyle.html
[in_enum]: salvo_oapi/openapi/path/enum.ParameterIn.html
[primitive]: https://doc.rust-lang.org/std/primitive/index.html
[serde attributes]: https://serde.rs/attributes.html
[to_schema_xml]: macro@ToSchema#xml-attribute-configuration-options