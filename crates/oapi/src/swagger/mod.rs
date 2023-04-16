//! This crate implements necessary boiler plate code to serve Swagger UI via web server. It
//! works as a bridge for serving the OpenAPI documentation created with [`salvo`][salvo] library in the
//! Swagger UI.
//!
//! [salvo]: <https://docs.rs/salvo/>
//!
//! ```
use std::{borrow::Cow, error::Error, mem, sync::Arc};

pub mod oauth;
mod config;
pub use config::Config;
use crate::openapi::OpenApi;
use indexmap::IndexMap;
use rust_embed::RustEmbed;
use salvo_core::http::{header, StatusError, HeaderValue, ResBody};
use salvo_core::{async_trait, Depot, FlowCtrl, Handler, Request, Response, Router};
use serde::Serialize;

#[derive(RustEmbed)]
#[folder = "$SALVO_SWAGGER_DIR/$SALVO_SWAGGER_UI_VERSION/dist/"]
struct SwaggerUiDist;

#[non_exhaustive]
#[derive(Clone,Debug)]
pub struct SwaggerUi {
    urls: Vec<(Url<'static>, OpenApi)>,
    config: Config<'static>,
    external_urls: Vec<(Url<'static>, serde_json::Value)>,
}
impl SwaggerUi {
    /// Create a new [`SwaggerUi`] for given path.
    ///
    /// Path argument will expose the Swagger UI to the user and should be something that
    /// the underlying application framework / library supports.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use salvo_oapi_swagger_ui::SwaggerUi;
    /// let swagger = SwaggerUi::new("/swagger-ui/{_:.*}");
    /// ```
    pub fn new(config: Config<'static>) -> Self {
        Self {
            urls: Vec::new(),
            config,
            external_urls: Vec::new(),
        }
    }

    /// Add api doc [`Url`] into [`SwaggerUi`].
    ///
    /// Method takes two arguments where first one is path which exposes the [`OpenApi`] to the user.
    /// Second argument is the actual Rust implementation of the OpenAPI doc which is being exposed.
    ///
    /// Calling this again will add another url to the Swagger UI.
    ///
    /// # Examples
    ///
    /// Expose manually created OpenAPI doc.
    /// ```rust
    /// # use salvo_oapi_swagger_ui::SwaggerUi;
    /// let swagger = SwaggerUi::new("/swagger-ui/{_:.*}")
    ///     .url("/api-docs/openapi.json", salvo_oapi::openapi::OpenApi::new(
    ///        salvo_oapi::openapi::Info::new("my application", "0.1.0"),
    ///        salvo_oapi::openapi::Paths::new(),
    /// ));
    /// ```
    ///
    /// Expose derived OpenAPI doc.
    /// ```rust
    /// # use salvo_oapi_swagger_ui::SwaggerUi;
    /// # use salvo_oapi::OpenApi;
    /// # #[derive(OpenApi)]
    /// # #[openapi()]
    /// # struct ApiDoc;
    /// let swagger = SwaggerUi::new("/swagger-ui/{_:.*}")
    ///     .url("/api-docs/openapi.json", ApiDoc::openapi());
    /// ```
    pub fn url<U: Into<Url<'static>>>(mut self, url: U, openapi: OpenApi) -> Self {
        self.urls.push((url.into(), openapi));

        self
    }

    /// Add multiple [`Url`]s to Swagger UI.
    ///
    /// Takes one [`Vec`] argument containing tuples of [`Url`] and [`OpenApi`].
    ///
    /// Situations where this comes handy is when there is a need or wish to separate different parts
    /// of the api to separate api docs.
    ///
    /// # Examples
    ///
    /// Expose multiple api docs via Swagger UI.
    /// ```rust
    /// # use salvo_oapi_swagger_ui::{SwaggerUi, Url};
    /// # use salvo_oapi::OpenApi;
    /// # #[derive(OpenApi)]
    /// # #[openapi()]
    /// # struct ApiDoc;
    /// # #[derive(OpenApi)]
    /// # #[openapi()]
    /// # struct ApiDoc2;
    /// let swagger = SwaggerUi::new("/swagger-ui/{_:.*}")
    ///     .urls(
    ///       vec![
    ///          (Url::with_primary("api doc 1", "/api-docs/openapi.json", true), ApiDoc::openapi()),
    ///          (Url::new("api doc 2", "/api-docs/openapi2.json"), ApiDoc2::openapi())
    ///     ]
    /// );
    /// ```
    pub fn urls(mut self, urls: Vec<(Url<'static>, OpenApi)>) -> Self {
        self.urls = urls;

        self
    }

    /// Add external API doc to the [`SwaggerUi`].
    ///
    /// This operation is unchecked and so it does not check any validity of provided content.
    /// Users are required to do their own check if any regarding validity of the external
    /// OpenAPI document.
    ///
    /// Method accepts two arguments, one is [`Url`] the API doc is served at and the second one is
    /// the [`serde_json::Value`] of the OpenAPI doc to be served.
    ///
    /// # Examples
    ///
    /// Add external API doc to the [`SwaggerUi`].
    ///```rust
    /// # use salvo_oapi_swagger_ui::{SwaggerUi, Url};
    /// # use salvo_oapi::OpenApi;
    /// # use serde_json::json;
    /// let external_openapi = json!({"openapi": "3.0.0"});
    ///
    /// let swagger = SwaggerUi::new("/swagger-ui/{_:.*}")
    ///     .external_url_unchecked("/api-docs/openapi.json", external_openapi);
    ///```
    pub fn external_url_unchecked<U: Into<Url<'static>>>(mut self, url: U, openapi: serde_json::Value) -> Self {
        self.external_urls.push((url.into(), openapi));

        self
    }

    /// Add external API docs to the [`SwaggerUi`] from iterator.
    ///
    /// This operation is unchecked and so it does not check any validity of provided content.
    /// Users are required to do their own check if any regarding validity of the external
    /// OpenAPI documents.
    ///
    /// Method accepts one argument, an `iter` of [`Url`] and [`serde_json::Value`] tuples. The
    /// [`Url`] will point to location the OpenAPI document is served and the [`serde_json::Value`]
    /// is the OpenAPI document to be served.
    ///
    /// # Examples
    ///
    /// Add external API docs to the [`SwaggerUi`].
    ///```rust
    /// # use salvo_oapi::swagger::{SwaggerUi, Url};
    /// # use salvo_oapi::OpenApi;
    /// # use serde_json::json;
    /// let external_openapi = json!({"openapi": "3.0.0"});
    /// let external_openapi2 = json!({"openapi": "3.0.0"});
    ///
    /// let swagger = SwaggerUi::new("/swagger-ui/{_:.*}")
    ///     .external_urls_from_iter_unchecked([
    ///         ("/api-docs/openapi.json", external_openapi),
    ///         ("/api-docs/openapi2.json", external_openapi2)
    ///     ]);
    ///```
    pub fn external_urls_from_iter_unchecked<I: IntoIterator<Item = (U, serde_json::Value)>, U: Into<Url<'static>>>(
        mut self,
        external_urls: I,
    ) -> Self {
        self.external_urls
            .extend(external_urls.into_iter().map(|(url, doc)| (url.into(), doc)));

        self
    }

    /// Add oauth [`oauth::Config`] into [`SwaggerUi`].
    ///
    /// Method takes one argument which exposes the [`oauth::Config`] to the user.
    ///
    /// # Examples
    ///
    /// Enable pkce with default client_id.
    /// ```rust
    /// # use salvo_oapi_swagger_ui::{SwaggerUi, oauth};
    /// # use salvo_oapi::OpenApi;
    /// # #[derive(OpenApi)]
    /// # #[openapi()]
    /// # struct ApiDoc;
    /// let swagger = SwaggerUi::new("/swagger-ui/{_:.*}")
    ///     .url("/api-docs/openapi.json", ApiDoc::openapi())
    ///     .oauth(oauth::Config::new()
    ///         .client_id("client-id")
    ///         .scopes(vec![String::from("openid")])
    ///         .use_pkce_with_authorization_code_grant(true)
    ///     );
    /// ```
    pub fn oauth(mut self, oauth: oauth::Config) -> Self {
        self.config.oauth = Some(oauth);
        self
    }

    pub fn router(self, path: impl Into<String>) -> Router {
        Router::with_path(format!("{}/<**>", path.into())).handle(self)
    }
}

#[async_trait]
impl Handler for SwaggerUi {
    async fn handle(&self, req: &mut Request, _depot: &mut Depot, res: &mut Response, _ctrl: &mut FlowCtrl) {
        let mut path = req.params().get("**").map(|s| &**s).unwrap_or_default();
        match (serve(path, &self.config)) {
            Ok(Some(file)) => {
                res.headers_mut()
                    .insert(header::CONTENT_TYPE, HeaderValue::from_str(&file.content_type).unwrap());
                res.set_body(ResBody::Once(file.bytes.to_vec().into()));
            }
            Ok(None) => {
                res.set_status_error(StatusError::not_found());
            }
            Err(e) => {
                tracing::error!(error = ?e, path =  "failed to fetch swagger ui file");
                res.set_status_error(StatusError::internal_server_error());
            }
        }
    }
}

/// Rust type for Swagger UI url configuration object.
#[non_exhaustive]
#[derive(Default, Serialize, Clone,Debug)]
pub struct Url<'a> {
    name: Cow<'a, str>,
    url: Cow<'a, str>,
    #[serde(skip)]
    primary: bool,
}

impl<'a> Url<'a> {
    /// Create new [`Url`].
    ///
    /// Name is shown in the select dropdown when there are multiple docs in Swagger UI.
    ///
    /// Url is path which exposes the OpenAPI doc.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use salvo_swagger_ui::Url;
    /// let url = Url::new("My Api", "/api-docs/openapi.json");
    /// ```
    pub fn new(name: &'a str, url: &'a str) -> Self {
        Self {
            name: Cow::Borrowed(name),
            url: Cow::Borrowed(url),
            ..Default::default()
        }
    }

    /// Create new [`Url`] with primary flag.
    ///
    /// Primary flag allows users to override the default behavior of the Swagger UI for selecting the primary
    /// doc to display. By default when there are multiple docs in Swagger UI the first one in the list
    /// will be the primary.
    ///
    /// Name is shown in the select dropdown when there are multiple docs in Swagger UI.
    ///
    /// Url is path which exposes the OpenAPI doc.
    ///
    /// # Examples
    ///
    /// Set "My Api" as primary.
    /// ```rust
    /// # use salvo_swagger_ui::Url;
    /// let url = Url::with_primary("My Api", "/api-docs/openapi.json", true);
    /// ```
    pub fn with_primary(name: &'a str, url: &'a str, primary: bool) -> Self {
        Self {
            name: Cow::Borrowed(name),
            url: Cow::Borrowed(url),
            primary,
        }
    }
}

impl<'a> From<&'a str> for Url<'a> {
    fn from(url: &'a str) -> Self {
        Self {
            url: Cow::Borrowed(url),
            ..Default::default()
        }
    }
}

impl From<String> for Url<'_> {
    fn from(url: String) -> Self {
        Self {
            url: Cow::Owned(url),
            ..Default::default()
        }
    }
}

impl<'a> From<Cow<'static, str>> for Url<'a> {
    fn from(url: Cow<'static, str>) -> Self {
        Self {
            url,
            ..Default::default()
        }
    }
}


/// Represents servable file of Swagger UI. This is used together with [`serve`] function
/// to serve Swagger UI files via web server.
#[non_exhaustive]
pub struct SwaggerFile<'a> {
    /// Content of the file as [`Cow`] [`slice`] of bytes.
    pub bytes: Cow<'a, [u8]>,
    /// Content type of the file e.g `"text/xml"`.
    pub content_type: String,
}

/// User friendly way to serve Swagger UI and its content via web server.
///
/// * **path** Should be the relative path to Swagger UI resource within the web server.
/// * **config** Swagger [`Config`] to use for the Swagger UI.
///
/// Typically this function is implemented _**within**_ handler what serves the Swagger UI. Handler itself must
/// match to user defined path that points to the root of the Swagger UI and match everything relatively
/// from the root of the Swagger UI _**(tail path)**_. The relative path from root of the Swagger UI
/// is used to serve [`SwaggerFile`]s. If Swagger UI is served from path `/swagger-ui/` then the `tail`
/// is everything under the `/swagger-ui/` prefix.
///
/// _There are also implementations in [examples of salvo repository][examples]._
///
/// [examples]: https://github.com/juhaku/salvo/tree/master/examples
pub fn serve<'a>(path: &str, config: &Config<'a>) -> Result<Option<SwaggerFile<'a>>, Box<dyn Error>> {
    let path  = if path.is_empty() || path == "/" {
        "index.html"
    } else {
        path
    };

println!("===========path: {}", path);
    if let Some(file) = SwaggerUiDist::get(path) {
        let mut bytes = file.data;

        if path == "swagger-initializer.js" {
            let mut file = match String::from_utf8(bytes.to_vec()) {
                Ok(file) => file,
                Err(error) => return Err(Box::new(error)),
            };

            file = format_config(config, file)?;

            if let Some(oauth) = &config.oauth {
                match oauth::format_swagger_config(oauth, file) {
                    Ok(oauth_file) => file = oauth_file,
                    Err(error) => return Err(Box::new(error)),
                }
            }

            bytes = Cow::Owned(file.as_bytes().to_vec())
        };

        Ok(Some(SwaggerFile {
            bytes,
            content_type: mime_guess::from_path(path).first_or_octet_stream().to_string(),
        }))
    } else {
        Ok(None)
    }
}
#[inline]
fn format_config(config: &Config, file: String) -> Result<String, Box<dyn Error>> {
    let config_json = match serde_json::to_string_pretty(&config) {
        Ok(config) => config,
        Err(error) => return Err(Box::new(error)),
    };

    // Replace {{config}} with pretty config json and remove the curly brackets `{ }` from beginning and the end.
    Ok(file.replace("{{config}}", &config_json[2..&config_json.len() - 2]))
}