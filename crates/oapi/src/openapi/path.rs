//! Implements [OpenAPI Path Object][paths] types.
//!
//! [paths]: https://spec.openapis.org/oas/latest.html#paths-object
use std::collections::BTreeMap;
use std::iter;
use std::ops::{Deref, DerefMut};

use serde::{Deserialize, Serialize};

use super::{set_value, Operation, Operations, Parameter, Parameters, Server, Servers};

/// Implements [OpenAPI Path Object][paths] types.
#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
pub struct Paths(BTreeMap<String, PathItem>);
impl Deref for Paths {
    type Target = BTreeMap<String, PathItem>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for Paths {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Paths {
    pub fn new() -> Self {
        Default::default()
    }
    pub fn path<K: Into<String>, V: Into<PathItem>>(mut self, key: K, value: V) -> Self {
        self.insert(key, value);
        self
    }
    pub fn insert<K: Into<String>, V: Into<PathItem>>(&mut self, key: K, value: V) {
        let key = key.into();
        let mut value = value.into();
        self.0
            .entry(key.clone())
            .and_modify(|item| {
                if !value.summary.is_none() {
                    item.summary = value.summary.take();
                }
                if !value.description.is_none() {
                    item.description = value.description.take();
                }
                item.servers.append(&mut value.servers);
                item.parameters.append(&mut value.parameters);
                item.operations.append(&mut value.operations);
            })
            .or_insert(value);
    }
    pub fn append(&mut self, other: &mut Paths) {
        let items = std::mem::replace(&mut other.0, Default::default());
        for item in items {
            self.insert(item.0, item.1);
        }
    }

    pub fn extend<I, K, V>(&mut self, iter: I)
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<PathItem>,
    {
        for (k, v) in iter.into_iter() {
            self.insert(k, v);
        }
    }
}

/// Implements [OpenAPI Path Item Object][path_item] what describes [`Operation`]s available on
/// a single path.
///
/// [path_item]: https://spec.openapis.org/oas/latest.html#path-item-object
#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PathItem {
    /// Optional summary intended to apply all operations in this [`PathItem`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    /// Optional description intended to apply all operations in this [`PathItem`].
    /// Description supports markdown syntax.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Alternative [`Server`] array to serve all [`Operation`]s in this [`PathItem`] overriding
    /// the global server array.
    #[serde(skip_serializing_if = "Servers::is_empty")]
    pub servers: Servers,

    /// List of [`Parameter`]s common to all [`Operation`]s in this [`PathItem`]. Parameters cannot
    /// contain duplicate parameters. They can be overridden in [`Operation`] level but cannot be
    /// removed there.
    #[serde(skip_serializing_if = "Parameters::is_empty")]
    #[serde(flatten)]
    pub parameters: Parameters,

    /// Map of operations in this [`PathItem`]. Operations can hold only one operation
    /// per [`PathItemType`].
    #[serde(flatten)]
    pub operations: Operations,
}

impl PathItem {
    /// Construct a new [`PathItem`] with provided [`Operation`] mapped to given [`PathItemType`].
    pub fn new<O: Into<Operation>>(path_item_type: PathItemType, operation: O) -> Self {
        let operations = BTreeMap::from_iter(iter::once((path_item_type, operation.into())));

        Self {
            operations: Operations(operations),
            ..Default::default()
        }
    }

    pub fn append(&mut self, other: &mut Self) {
        self.operations.append(&mut other.operations);
        self.servers.append(&mut other.servers);
        self.parameters.append(&mut other.parameters);
        if other.description.is_none() {
            self.description = other.description.take();
        }
        if !other.summary.is_none() {
            self.summary = other.summary.take();
        }
    }

    /// Append a new [`Operation`] by [`PathItemType`] to this [`PathItem`]. Operations can
    /// hold only one operation per [`PathItemType`].
    pub fn add_operation<O: Into<Operation>>(mut self, path_item_type: PathItemType, operation: O) -> Self {
        self.operations.insert(path_item_type, operation.into());

        self
    }

    /// Add or change summary intended to apply all operations in this [`PathItem`].
    pub fn summary<S: Into<String>>(mut self, summary: S) -> Self {
        set_value!(self summary Some(summary.into()))
    }

    /// Add or change optional description intended to apply all operations in this [`PathItem`].
    /// Description supports markdown syntax.
    pub fn description<S: Into<String>>(mut self, description: S) -> Self {
        set_value!(self description Some(description.into()))
    }

    /// Add list of alternative [`Server`]s to serve all [`Operation`]s in this [`PathItem`] overriding
    /// the global server array.
    pub fn servers<I: IntoIterator<Item = Server>>(mut self, servers: I) -> Self {
        set_value!(self servers Servers(servers.into_iter().collect()))
    }

    /// Append list of [`Parameter`]s common to all [`Operation`]s to this [`PathItem`].
    pub fn parameters<I: IntoIterator<Item = Parameter>>(mut self, parameters: I) -> Self {
        set_value!(self parameters Parameters(parameters.into_iter().collect()))
    }
}

/// Path item operation type.
#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord, Clone, Copy, Debug)]
#[serde(rename_all = "lowercase")]
pub enum PathItemType {
    /// Type mapping for HTTP _GET_ request.
    Get,
    /// Type mapping for HTTP _POST_ request.
    Post,
    /// Type mapping for HTTP _PUT_ request.
    Put,
    /// Type mapping for HTTP _DELETE_ request.
    Delete,
    /// Type mapping for HTTP _OPTIONS_ request.
    Options,
    /// Type mapping for HTTP _HEAD_ request.
    Head,
    /// Type mapping for HTTP _PATCH_ request.
    Patch,
    /// Type mapping for HTTP _TRACE_ request.
    Trace,
    /// Type mapping for HTTP _CONNECT_ request.
    Connect,
}
