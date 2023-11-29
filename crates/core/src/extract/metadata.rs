use std::str::FromStr;
use std::vec;

use cruet::Inflector;

use self::RenameRule::*;

/// Source for a field.
#[derive(Eq, PartialEq, Copy, Clone, Debug)]
#[non_exhaustive]
pub enum SourceFrom {
    /// The field will extracted from url param.
    Param,
    /// The field will extracted from url query.
    Query,
    /// The field will extracted from http header.
    Header,
    /// The field will extracted from http cookie.
    #[cfg(feature = "cookie")]
    Cookie,
    /// The field will extracted from http payload.
    Body,
}

impl FromStr for SourceFrom {
    type Err = crate::Error;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "param" => Ok(Self::Param),
            "query" => Ok(Self::Query),
            "header" => Ok(Self::Header),
            #[cfg(feature = "cookie")]
            "cookie" => Ok(Self::Cookie),
            "body" => Ok(Self::Body),
            _ => Err(crate::Error::Other(format!("invalid source from `{input}`").into())),
        }
    }
}

/// Rename rule for a field.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
#[non_exhaustive]
pub enum RenameRule {
    /// Rename direct children to "lowercase" style.
    LowerCase,
    /// Rename direct children to "UPPERCASE" style.
    UpperCase,
    /// Rename direct children to "PascalCase" style, as typically used for
    /// enum variants.
    PascalCase,
    /// Rename direct children to "camelCase" style.
    CamelCase,
    /// Rename direct children to "snake_case" style, as commonly used for
    /// fields.
    SnakeCase,
    /// Rename direct children to "SCREAMING_SNAKE_CASE" style, as commonly
    /// used for constants.
    ScreamingSnakeCase,
    /// Rename direct children to "kebab-case" style.
    KebabCase,
    /// Rename direct children to "SCREAMING-KEBAB-CASE" style.
    ScreamingKebabCase,
}

impl FromStr for RenameRule {
    type Err = crate::Error;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        for (name, rule) in RENAME_RULES {
            if input == *name {
                return Ok(*rule);
            }
        }
        Err(crate::Error::other(format!("invalid rename rule: {input}")))
    }
}

static RENAME_RULES: &[(&str, RenameRule)] = &[
    ("lowercase", LowerCase),
    ("UPPERCASE", UpperCase),
    ("PascalCase", PascalCase),
    ("camelCase", CamelCase),
    ("snake_case", SnakeCase),
    ("SCREAMING_SNAKE_CASE", ScreamingSnakeCase),
    ("kebab-case", KebabCase),
    ("SCREAMING-KEBAB-CASE", ScreamingKebabCase),
];
impl RenameRule {
    /// Apply a renaming rule to an variant, returning the version expected in the source.
    pub fn rename(&self, name: impl AsRef<str>) -> String {
        let name = name.as_ref();
        match *self {
            PascalCase => name.to_pascal_case(),
            LowerCase => name.to_lowercase(),
            UpperCase => name.to_uppercase(),
            CamelCase => name.to_camel_case(),
            SnakeCase => name.to_snake_case(),
            ScreamingSnakeCase => SnakeCase.rename(name).to_ascii_uppercase(),
            KebabCase => SnakeCase.rename(name).replace('_', "-"),
            ScreamingKebabCase => ScreamingSnakeCase.rename(name).replace('_', "-"),
        }
    }
}

/// Source parser for a source.
///
/// This parser is used to parse field data, not the request mime type.
/// For example, if request is posted as form, but the field is string as json format, it can be parsed as json.
#[derive(Eq, PartialEq, Copy, Clone, Debug)]
#[non_exhaustive]
pub enum SourceParser {
    /// MulitMap parser.
    MultiMap,
    /// Json parser.
    Json,
    /// Smart parser.
    Smart,
}

impl FromStr for SourceParser {
    type Err = crate::Error;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "multimap" => Ok(Self::MultiMap),
            "json" => Ok(Self::Json),
            "smart" => Ok(Self::Smart),
            _ => Err(crate::Error::Other("invalid source format".into())),
        }
    }
}

/// Struct's metadata information.
#[derive(Default, Clone, Debug)]
#[non_exhaustive]
pub struct Metadata {
    /// The name of this type.
    pub name: &'static str,
    /// Default sources of all fields.
    pub default_sources: Vec<Source>,
    /// Fields of this type.
    pub fields: Vec<Field>,
    /// Rename rule for all fields of this type.
    pub rename_all: Option<RenameRule>,
}

impl Metadata {
    /// Create a new metadata object.
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            default_sources: vec![],
            fields: vec![],
            rename_all: None,
        }
    }

    /// Sets the default sources list to a new value.
    pub fn default_sources(mut self, default_sources: Vec<Source>) -> Self {
        self.default_sources = default_sources;
        self
    }

    /// set all fields list to a new value.
    pub fn fields(mut self, fields: Vec<Field>) -> Self {
        self.fields = fields;
        self
    }

    /// Add a default source to default sources list.
    pub fn add_default_source(mut self, source: Source) -> Self {
        self.default_sources.push(source);
        self
    }

    /// Add a field to the fields list.
    pub fn add_field(mut self, field: Field) -> Self {
        self.fields.push(field);
        self
    }

    /// Rule for rename all fields of type.
    pub fn rename_all(mut self, rename_all: impl Into<Option<RenameRule>>) -> Self {
        self.rename_all = rename_all.into();
        self
    }

    /// Check is this type has body required.
    pub(crate) fn has_body_required(&self) -> bool {
        if self.default_sources.iter().any(|s| s.from == SourceFrom::Body) {
            return true;
        }
        self.fields.iter().any(|f| f.has_body_required())
    }
}

/// Information about struct field.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Field {
    /// Field name.
    pub name: &'static str,
    /// Field flatten, this field will extracted from request.
    pub flatten: bool,
    /// Field sources.
    pub sources: Vec<Source>,
    /// Field aliaes.
    pub aliases: Vec<&'static str>,
    /// Field rename.
    pub rename: Option<&'static str>,
    /// Field metadata, this is used for nested extractible types.
    pub metadata: Option<&'static Metadata>,
}
impl Field {
    /// Create a new field with the given name and kind.
    pub fn new(name: &'static str) -> Self {
        Self::with_sources(name, vec![])
    }

    /// Create a new field with the given name and kind, and the given sources.
    pub fn with_sources(name: &'static str, sources: Vec<Source>) -> Self {
        Self {
            name,
            flatten: false,
            sources,
            aliases: vec![],
            rename: None,
            metadata: None,
        }
    }

    /// Sets the flatten to the given value.
    pub fn set_flatten(mut self, flatten: bool) -> Self {
        self.flatten = flatten;
        self
    }

    /// Sets the metadata to the field type.
    pub fn metadata(mut self, metadata: &'static Metadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Add a source to sources list.
    pub fn add_source(mut self, source: Source) -> Self {
        self.sources.push(source);
        self
    }

    /// Sets the aliases list to a new value.
    pub fn set_aliases(mut self, aliases: Vec<&'static str>) -> Self {
        self.aliases = aliases;
        self
    }

    /// Add a alias to aliases list.
    pub fn add_alias(mut self, alias: &'static str) -> Self {
        self.aliases.push(alias);
        self
    }

    /// Sets the rename to the given value.
    pub fn rename(mut self, rename: &'static str) -> Self {
        self.rename = Some(rename);
        self
    }

    /// Check is this field has body required.
    pub(crate) fn has_body_required(&self) -> bool {
        self.sources.iter().any(|s| s.from == SourceFrom::Body)
    }
}

/// Request source for extract data.
#[derive(Copy, Clone, Debug)]
#[non_exhaustive]
pub struct Source {
    /// The source from.
    pub from: SourceFrom,
    /// The parser used to parse data.
    pub parser: SourceParser,
}
impl Source {
    /// Create a new source from a string.
    pub fn new(from: SourceFrom, parser: SourceParser) -> Self {
        Self { from, parser }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_source_from() {
        for (key, value) in [
            ("param", SourceFrom::Param),
            ("query", SourceFrom::Query),
            ("header", SourceFrom::Header),
            #[cfg(feature = "cookie")]
            ("cookie", SourceFrom::Cookie),
            ("body", SourceFrom::Body),
        ] {
            assert_eq!(key.parse::<SourceFrom>().unwrap(), value);
        }
        assert!("abcd".parse::<SourceFrom>().is_err());
    }

    #[test]
    fn test_parse_source_format() {
        for (key, value) in [("multimap", SourceParser::MultiMap), ("json", SourceParser::Json)] {
            assert_eq!(key.parse::<SourceParser>().unwrap(), value);
        }
        assert!("abcd".parse::<SourceParser>().is_err());
    }

    #[test]
    fn test_parse_rename_rule() {
        for (key, value) in RENAME_RULES {
            assert_eq!(key.parse::<RenameRule>().unwrap(), *value);
        }
        assert!("abcd".parse::<RenameRule>().is_err());
    }

    #[test]
    fn test_rename_rule() {
        assert_eq!(PascalCase.rename("rename_rule"), "RenameRule");
        assert_eq!(LowerCase.rename("RenameRule"), "renamerule");
        assert_eq!(UpperCase.rename("rename_rule"), "RENAME_RULE");
        assert_eq!(CamelCase.rename("RenameRule"), "renameRule");
        assert_eq!(SnakeCase.rename("RenameRule"), "rename_rule");
        assert_eq!(ScreamingSnakeCase.rename("rename_rule"), "RENAME_RULE");
        assert_eq!(KebabCase.rename("rename_rule"), "rename-rule");
        assert_eq!(ScreamingKebabCase.rename("rename_rule"), "RENAME-RULE");
    }
}
