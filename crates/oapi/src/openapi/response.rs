//! Implements [OpenApi Responses][responses].
//!
//! [responses]: https://spec.openapis.org/oas/latest.html#responses-object
use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::AsResponses;
use crate::{Ref, RefOr};

use super::{header::Header, set_value, Content};

/// Implements [OpenAPI Responses Object][responses].
///
/// Responses is a map holding api operation responses identified by their status code.
///
/// [responses]: https://spec.openapis.org/oas/latest.html#responses-object
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Responses(BTreeMap<String, RefOr<Response>>);

impl Deref for Responses {
    type Target = BTreeMap<String, RefOr<Response>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Responses {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Responses {
    pub fn new() -> Self {
        Default::default()
    }
    /// Add a [`Response`].
    pub fn response<S: Into<String>, R: Into<RefOr<Response>>>(mut self, code: S, response: R) -> Self {
        self.insert(code, response);
        self
    }

    pub fn insert<S: Into<String>, R: Into<RefOr<Response>>>(&mut self, code: S, response: R) {
        self.0.insert(code.into(), response.into());
    }
    pub fn append(&mut self, other: &mut Responses) {
        other.0.append(&mut self.0);
        std::mem::swap(&mut self.0, &mut other.0);
    }

    /// Add responses from an iterator over a pair of `(status_code, response): (String, Response)`.
    pub fn extend<I, C, R>(mut self, iter: I)
    where
        I: IntoIterator<Item = (C, R)>,
        C: Into<String>,
        R: Into<RefOr<Response>>,
    {
        self.0
            .extend(iter.into_iter().map(|(code, response)| (code.into(), response.into())));
    }

    /// Add responses from a type that implements [`AsResponses`].
    pub fn responses_from_as_responses<I: AsResponses>(mut self) -> Self {
        self.0.extend(I::responses());
        self
    }
}

impl From<Responses> for BTreeMap<String, RefOr<Response>> {
    fn from(responses: Responses) -> Self {
        responses.0
    }
}

impl<C, R> FromIterator<(C, R)> for Responses
where
    C: Into<String>,
    R: Into<RefOr<Response>>,
{
    fn from_iter<T: IntoIterator<Item = (C, R)>>(iter: T) -> Self {
        Self(BTreeMap::from_iter(
            iter.into_iter().map(|(code, response)| (code.into(), response.into())),
        ))
    }
}

/// Implements [OpenAPI Response Object][response].
///
/// Response is api operation response.
///
/// [response]: https://spec.openapis.org/oas/latest.html#response-object
#[non_exhaustive]
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    /// Description of the response. Response support markdown syntax.
    pub description: String,

    /// Map of headers identified by their name. `Content-Type` header will be ignored.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub headers: BTreeMap<String, Header>,

    /// Map of response [`Content`] objects identified by response body content type e.g `application/json`.
    ///
    /// [`Content`]s are stored within [`IndexMap`] to retain their insertion order. Swagger UI
    /// will create and show default example according to the first entry in `content` map.
    #[serde(skip_serializing_if = "IndexMap::is_empty", default)]
    pub content: IndexMap<String, Content>,
}

impl Response {
    /// Construct a new [`Response`].
    ///
    /// Function takes description as argument.
    pub fn new<S: Into<String>>(description: S) -> Self {
        Self {
            description: description.into(),
            ..Default::default()
        }
    }
    /// Add description. Description supports markdown syntax.
    pub fn description<I: Into<String>>(mut self, description: I) -> Self {
        set_value!(self description description.into())
    }

    /// Add [`Content`] of the [`Response`] with content type e.g `application/json`.
    pub fn content<S: Into<String>>(mut self, content_type: S, content: Content) -> Self {
        self.content.insert(content_type.into(), content);

        self
    }

    /// Add response [`Header`].
    pub fn header<S: Into<String>>(mut self, name: S, header: Header) -> Self {
        self.headers.insert(name.into(), header);

        self
    }
}

impl From<Ref> for RefOr<Response> {
    fn from(r: Ref) -> Self {
        Self::Ref(r)
    }
}

#[cfg(test)]
mod tests {
    use super::{Content, Response, Responses};
    use assert_json_diff::assert_json_eq;
    use serde_json::json;

    #[test]
    fn responses_new() {
        let responses = Responses::new();
        assert!(responses.is_empty());
    }

    #[test]
    fn response_builder() -> Result<(), serde_json::Error> {
        let request_body = Response::new("A sample response").content(
            "application/json",
            Content::new(crate::Ref::from_schema_name("MySchemaPayload")),
        );
        let serialized = serde_json::to_string_pretty(&request_body)?;
        println!("serialized json:\n {serialized}");
        assert_json_eq!(
            request_body,
            json!({
              "description": "A sample response",
              "content": {
                "application/json": {
                  "schema": {
                    "$ref": "#/components/schemas/MySchemaPayload"
                  }
                }
              }
            })
        );
        Ok(())
    }
}
