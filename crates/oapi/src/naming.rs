use std::any::TypeId;
use std::collections::BTreeMap;
use std::sync::LazyLock;

use parking_lot::{RwLock, RwLockReadGuard};
use regex::Regex;

/// NameRule is used to specify the rule of naming.
#[derive(Default, Debug, Clone, Copy)]
pub enum NameRule {
    /// Auto generate name by namer.
    #[default]
    Auto,
    /// Force to use the given name.
    Force(&'static str),
}

static GLOBAL_NAMER: LazyLock<RwLock<Box<dyn Namer>>> =
    LazyLock::new(|| RwLock::new(Box::new(FlexNamer::new())));
static NAME_TYPES: LazyLock<RwLock<BTreeMap<String, (TypeId, &'static str)>>> =
    LazyLock::new(Default::default);

/// Set global namer.
///
/// Set global namer, all the types will be named by this namer. You should call this method before
/// at before you generate OpenAPI schema.
///
/// # Example
///
/// ```rust
/// # use salvo_oapi::extract::*;
/// # use salvo_core::prelude::*;
/// # #[tokio::main]
/// # async fn main() {
///     salvo_oapi::naming::set_namer(salvo_oapi::naming::FlexNamer::new().short_mode(true).generic_delimiter('_', '_'));
/// # }
/// ```
pub fn set_namer(namer: impl Namer) {
    *GLOBAL_NAMER.write() = Box::new(namer);
    NAME_TYPES.write().clear();
}

#[doc(hidden)]
pub fn namer() -> RwLockReadGuard<'static, Box<dyn Namer>> {
    GLOBAL_NAMER.read()
}

/// Get type info by name.
pub fn type_info_by_name(name: &str) -> Option<(TypeId, &'static str)> {
    NAME_TYPES.read().get(name).cloned()
}

/// Set type info by name.
pub fn set_name_type_info(
    name: String,
    type_id: TypeId,
    type_name: &'static str,
) -> Option<(TypeId, &'static str)> {
    NAME_TYPES
        .write()
        .insert(name.clone(), (type_id, type_name))
}

/// Assign name to type and returns the name.
///
/// If the type is already named, return the existing name.
pub fn assign_name<T: 'static>(rule: NameRule) -> String {
    let type_id = TypeId::of::<T>();
    let type_name = std::any::type_name::<T>();
    for (name, (exist_id, _)) in NAME_TYPES.read().iter() {
        if *exist_id == type_id {
            return name.clone();
        }
    }
    namer().assign_name(type_id, type_name, rule)
}

/// Get the name of the type. Panic if the name is not exist.
pub fn get_name<T: 'static>() -> String {
    let type_id = TypeId::of::<T>();
    for (name, (exist_id, _)) in NAME_TYPES.read().iter() {
        if *exist_id == type_id {
            return name.clone();
        }
    }
    panic!(
        "Type not found in the name registry: {:?}",
        std::any::type_name::<T>()
    );
}

fn type_generic_part(type_name: &str) -> String {
    let re = Regex::new(r"^[^<]+").expect("Invalid regex");
    let result = re.replace_all(type_name, "");
    result.to_string()
}
/// Namer is used to assign names to types.
pub trait Namer: Sync + Send + 'static {
    /// Assign name to type.
    fn assign_name(&self, type_id: TypeId, type_name: &'static str, rule: NameRule) -> String;
}

/// A namer that generates wordy names.
#[derive(Default, Clone, Debug)]
pub struct FlexNamer {
    short_mode: bool,
    generic_delimiter: Option<(String, String)>,
}
impl FlexNamer {
    /// Create a new FlexNamer.
    pub fn new() -> Self {
        Default::default()
    }

    /// Set the short mode.
    pub fn short_mode(mut self, short_mode: bool) -> Self {
        self.short_mode = short_mode;
        self
    }

    /// Set the delimiter for generic types.
    pub fn generic_delimiter(mut self, open: impl Into<String>, close: impl Into<String>) -> Self {
        self.generic_delimiter = Some((open.into(), close.into()));
        self
    }
}
impl Namer for FlexNamer {
    fn assign_name(&self, type_id: TypeId, type_name: &'static str, rule: NameRule) -> String {
        let name = match rule {
            NameRule::Auto => {
                let mut base = if self.short_mode {
                    let re = Regex::new(r"([^<>]*::)+").expect("Invalid regex");
                    re.replace_all(type_name, "").to_string()
                } else {
                    type_name.replace("::", ".")
                };
                if let Some((open, close)) = &self.generic_delimiter {
                    base = base.replace('<', open).replace('>', close);
                }
                let mut name = base.to_string();
                let mut count = 1;
                while let Some(exist_id) = type_info_by_name(&name).map(|t| t.0) {
                    if exist_id != type_id {
                        count += 1;
                        name = format!("{}{}", base, count);
                    } else {
                        break;
                    }
                }
                name
            }
            NameRule::Force(force_name) => {
                let mut base = if self.short_mode {
                    let re = Regex::new(r"([^<>]*::)+").expect("Invalid regex");
                    re.replace_all(force_name, "").to_string()
                } else {
                    format! {"{}{}", force_name, type_generic_part(type_name).replace("::", ".")}
                };
                if let Some((open, close)) = &self.generic_delimiter {
                    base = base.replace('<', open).replace('>', close).to_string();
                }
                let mut name = base.to_string();
                let mut count = 1;
                while let Some((exist_id, exist_name)) = type_info_by_name(&name) {
                    if exist_id != type_id {
                        count += 1;
                        tracing::error!("Duplicate name for types: {}, {}", exist_name, type_name);
                        name = format!("{}{}", base, count);
                    } else {
                        break;
                    }
                }
                name.to_string()
            }
        };
        set_name_type_info(name.clone(), type_id, type_name);
        name
    }
}

mod tests {
    #[test]
    fn test_name() {
        use super::*;

        struct MyString;
        mod nest {
            pub(crate) struct MyString;
        }

        let name = assign_name::<String>(NameRule::Auto);
        assert_eq!(name, "alloc.string.String");
        let name = assign_name::<Vec<String>>(NameRule::Auto);
        assert_eq!(name, "alloc.vec.Vec<alloc.string.String>");

        let name = assign_name::<MyString>(NameRule::Auto);
        assert_eq!(name, "salvo_oapi.naming.tests.test_name.MyString");
        let name = assign_name::<nest::MyString>(NameRule::Auto);
        assert_eq!(name, "salvo_oapi.naming.tests.test_name.nest.MyString");

        // let namer = FlexNamer::new().generic_delimiter('_', '_');
        // set_namer(namer);

        // let name = assign_name::<String>(NameRule::Auto);
        // assert_eq!(name, "alloc.string.String");
        // let name = assign_name::<Vec<String>>(NameRule::Auto);
        // assert_eq!(name, "alloc.vec.Vec_alloc.string.String_");

        // let namer = FlexNamer::new().short_mode(true).generic_delimiter('_', '_');
        // set_namer(namer);

        // let name = assign_name::<String>(NameRule::Auto);
        // assert_eq!(name, "String");
        // let name = assign_name::<Vec<String>>(NameRule::Auto);
        // assert_eq!(name, "Vec_String_");

        // let namer = FlexNamer::new().short_mode(true).generic_delimiter('_', '_');
        // set_namer(namer);

        // struct MyString;
        // mod nest {
        //     pub(crate) struct MyString;
        // }

        // let name = assign_name::<MyString>(NameRule::Auto);
        // assert_eq!(name, "MyString");
        // let name = assign_name::<nest::MyString>(NameRule::Auto);
        // assert_eq!(name, "MyString2");
    }
}
