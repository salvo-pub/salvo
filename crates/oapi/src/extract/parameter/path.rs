use std::fmt::{self, Formatter};
use std::ops::{Deref, DerefMut};

use salvo_core::extract::{Extractible, Metadata};
use salvo_core::http::{ParseError, Request};
use serde::{Deserialize, Deserializer};

use crate::endpoint::EndpointArgRegister;
use crate::{Components, Operation, Parameter, ParameterIn, ToSchema};

/// Represents the parameters passed by the URI path.
pub struct PathParam<T>(pub T);
impl<T> PathParam<T> {
    /// Consumes self and returns the value of the parameter.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> Deref for PathParam<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for PathParam<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<'de, T> Deserialize<'de> for PathParam<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        T::deserialize(deserializer).map(|value| PathParam(value))
    }
}

impl<T> fmt::Debug for PathParam<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<T> fmt::Display for PathParam<T>
where
    T: fmt::Display,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<'ex, T> Extractible<'ex> for PathParam<T>
where
    T: Deserialize<'ex>,
{
    fn metadata() -> &'ex Metadata {
        static METADATA: Metadata = Metadata::new("");
        &METADATA
    }
    #[allow(refining_impl_trait)]
    async fn extract(_req: &'ex mut Request) -> Result<Self, ParseError> {
        unimplemented!("path parameter can not be extracted from request")
    }
    #[allow(refining_impl_trait)]
    async fn extract_with_arg(req: &'ex mut Request, arg: &str) -> Result<Self, ParseError> {
        let value = req
            .param(arg)
            .ok_or_else(|| ParseError::other(format!("path parameter {} not found or convert to type failed", arg)))?;
        Ok(Self(value))
    }
}

impl<T> EndpointArgRegister for PathParam<T>
where
    T: ToSchema,
{
    fn register(components: &mut Components, operation: &mut Operation, arg: &str) {
        let parameter = Parameter::new(arg)
            .parameter_in(ParameterIn::Path)
            .description(format!("Get parameter `{arg}` from request url path."))
            .schema(T::to_schema(components))
            .required(true);
        operation.parameters.insert(parameter);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_param_into_inner() {
        let param = PathParam::<String>("param".to_string());
        assert_eq!("param".to_string(), param.into_inner());
    }

    #[test]
    fn test_path_param_deref() {
        let param = PathParam::<String>("param".to_string());
        assert_eq!(&"param".to_string(), param.deref())
    }

    #[test]
    fn test_path_param_deref_mut() {
        let mut param = PathParam::<String>("param".to_string());
        assert_eq!(&mut "param".to_string(), param.deref_mut())
    }

    #[test]
    fn test_path_param_deserialize() {
        let param = serde_json::from_str::<PathParam<String>>(r#""param""#).unwrap();
        assert_eq!(param.0, "param");
    }

    #[test]
    fn test_path_param_debug() {
        let param = PathParam::<String>("param".to_string());
        assert_eq!(format!("{:?}", param), r#""param""#);
    }

    #[test]
    fn test_path_param_display() {
        let param = PathParam::<String>("param".to_string());
        assert_eq!(format!("{}", param), "param");
    }
}
