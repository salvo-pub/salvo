use std::fmt::{self, Formatter};
use std::ops::{Deref, DerefMut};

use salvo_core::extract::{Extractible, Metadata};
use salvo_core::http::ParseError;
use salvo_core::serde::from_str_val;
use salvo_core::{async_trait, Request};
use serde::Deserialize;
use serde::Deserializer;

use crate::endpoint::EndpointArgRegister;
use crate::{Components, Operation, Parameter, ParameterIn, ToParameter};

/// Represents the parameters passed by Cookie.
pub struct CookieParam<T> {
    name: String,
    value: T,
}
impl<T> CookieParam<T> {
    /// Construct a new [`CookieParam`] with given `name` and `value`.
    pub fn new(name: &str, value: T) -> Self {
        Self {
            name: name.into(),
            value,
        }
    }
    /// Returns the name of the parameter.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Returns the value of the parameter.
    pub fn value(&self) -> &T {
        &self.value
    }
}

impl<T> Deref for CookieParam<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T> DerefMut for CookieParam<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

impl<'de, T> Deserialize<'de> for CookieParam<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        T::deserialize(deserializer).map(|value| CookieParam {
            name: "unknown".into(),
            value,
        })
    }
}

impl<T> fmt::Debug for CookieParam<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("CookieParam")
            .field("name", &self.name)
            .field("value", &self.value)
            .finish()
    }
}

#[async_trait]
impl<'de, T> Extractible<'de> for CookieParam<T>
where
    T: Deserialize<'de>,
{
    fn metadata() -> &'de Metadata {
        static METADATA: Metadata = Metadata::new("");
        &METADATA
    }
    async fn extract(_req: &'de mut Request) -> Result<Self, ParseError> {
        unimplemented!("cookie parameter can not be extracted from request")
    }
    async fn extract_with_arg(req: &'de mut Request, arg: &str) -> Result<Self, ParseError> {
        let value = req
            .cookies()
            .get(arg)
            .and_then(|v| from_str_val(v.value()).ok())
            .ok_or_else(|| {
                ParseError::other(format!("cookie parameter {} not found or convert to type failed", arg))
            })?;
        Ok(Self {
            name: arg.to_string(),
            value,
        })
    }
}

impl<T> EndpointArgRegister for CookieParam<T> {
    fn register(_components: &mut Components, operation: &mut Operation, arg: &str) {
        let parameter = Parameter::new(arg)
            .parameter_in(ParameterIn::Cookie)
            .description(format!("Get parameter `{arg}` from request cookie"));
        operation.parameters.insert(parameter);
    }
}
