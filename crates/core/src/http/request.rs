//! Http request.

use std::fmt::{self, Formatter};

use bytes::Bytes;
#[cfg(feature = "cookie")]
use cookie::{Cookie, CookieJar};
use http::header::{AsHeaderName, HeaderMap, HeaderValue, IntoHeaderName, CONTENT_TYPE};
use http::method::Method;
pub use http::request::Parts;
use http::uri::{Scheme, Uri};
use http::{self, Extensions};
use http_body_util::{BodyExt, Limited};
use indexmap::IndexMap;
use mime;
use multimap::MultiMap;
use once_cell::sync::OnceCell;
use parking_lot::RwLock;
use serde::de::Deserialize;

use crate::conn::SocketAddr;
use crate::extract::{Extractible, Metadata};
use crate::http::body::ReqBody;
use crate::http::form::{FilePart, FormData};
use crate::http::{Mime, ParseError, Version};
use crate::serde::{from_request, from_str_map, from_str_multi_map, from_str_multi_val, from_str_val};
use crate::Error;

static SECURE_MAX_SIZE: RwLock<usize> = RwLock::new(64 * 1024);

/// Get global secure max size, default value is 64KB.
pub fn secure_max_size() -> usize {
    *SECURE_MAX_SIZE.read()
}

/// Set secure max size globally.
pub fn set_secure_max_size(size: usize) {
    let mut lock = SECURE_MAX_SIZE.write();
    *lock = size;
}

/// Represents an HTTP request.
///
/// Stores all the properties of the client's request.
pub struct Request {
    // The requested URL.
    uri: Uri,

    // The request headers.
    headers: HeaderMap,

    // The request body as a reader.
    body: ReqBody,
    pub(crate) extensions: Extensions,

    // The request method.
    method: Method,

    #[cfg(feature = "cookie")]
    pub(crate) cookies: CookieJar,

    pub(crate) params: IndexMap<String, String>,

    // accept: Option<Vec<Mime>>,
    pub(crate) queries: OnceCell<MultiMap<String, String>>,
    pub(crate) form_data: tokio::sync::OnceCell<FormData>,
    pub(crate) payload: tokio::sync::OnceCell<Bytes>,

    /// The version of the HTTP protocol used.
    pub(crate) version: Version,
    pub(crate) scheme: Scheme,
    pub(crate) local_addr: SocketAddr,
    pub(crate) remote_addr: SocketAddr,
}

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("Request")
            .field("method", self.method())
            .field("uri", self.uri())
            .field("version", &self.version())
            .field("scheme", &self.scheme())
            .field("headers", self.headers())
            // omits Extensions because not useful
            .field("body", &self.body())
            .field("local_addr", &self.local_addr)
            .field("remote_addr", &self.remote_addr)
            .finish()
    }
}

impl Default for Request {
    #[inline]
    fn default() -> Request {
        Request::new()
    }
}

impl Request {
    /// Creates a new blank `Request`
    #[inline]
    pub fn new() -> Request {
        Request {
            uri: Uri::default(),
            headers: HeaderMap::default(),
            body: ReqBody::default(),
            extensions: Extensions::default(),
            method: Method::default(),
            #[cfg(feature = "cookie")]
            cookies: CookieJar::default(),
            params: IndexMap::new(),
            queries: OnceCell::new(),
            form_data: tokio::sync::OnceCell::new(),
            payload: tokio::sync::OnceCell::new(),
            version: Version::default(),
            scheme: Scheme::HTTP,
            local_addr: SocketAddr::Unknown,
            remote_addr: SocketAddr::Unknown,
        }
    }
    /// Creates a new `Request` from [`hyper::Request`].
    pub fn from_hyper<B>(req: hyper::Request<B>, scheme: Scheme) -> Self
    where
        B: Into<ReqBody>,
    {
        let (
            http::request::Parts {
                method,
                uri,
                version,
                headers,
                extensions,
                ..
            },
            body,
        ) = req.into_parts();

        // Set the request cookies, if they exist.
        #[cfg(feature = "cookie")]
        let cookies = if let Some(header) = headers.get("Cookie") {
            let mut cookie_jar = CookieJar::new();
            if let Ok(header) = header.to_str() {
                for cookie_str in header.split(';').map(|s| s.trim()) {
                    if let Ok(cookie) = Cookie::parse_encoded(cookie_str).map(|c| c.into_owned()) {
                        cookie_jar.add_original(cookie);
                    }
                }
            }
            cookie_jar
        } else {
            CookieJar::new()
        };

        Request {
            queries: OnceCell::new(),
            uri,
            headers,
            body: body.into(),
            extensions,
            method,
            #[cfg(feature = "cookie")]
            cookies,
            // accept: None,
            params: IndexMap::new(),
            form_data: tokio::sync::OnceCell::new(),
            payload: tokio::sync::OnceCell::new(),
            // multipart: OnceCell::new(),
            local_addr: SocketAddr::Unknown,
            remote_addr: SocketAddr::Unknown,
            version,
            scheme,
        }
    }

    /// Strip the request to [`hyper::Request`].
    #[doc(hidden)]
    #[inline]
    pub fn strip_to_hyper(&mut self) -> Result<hyper::Request<ReqBody>, http::Error> {
        let mut builder = http::request::Builder::new()
            .method(self.method.clone())
            .uri(self.uri.clone())
            .version(self.version);
        if let Some(headers) = builder.headers_mut() {
            *headers = std::mem::take(&mut self.headers);
        }
        if let Some(extensions) = builder.extensions_mut() {
            *extensions = std::mem::take(&mut self.extensions);
        }
        builder.body(std::mem::take(&mut self.body))
    }

    /// Merge data from [`hyper::Request`].
    #[doc(hidden)]
    #[inline]
    pub fn merge_hyper(&mut self, hyper_req: hyper::Request<ReqBody>) {
        let (
            http::request::Parts {
                method,
                uri,
                version,
                headers,
                extensions,
                ..
            },
            body,
        ) = hyper_req.into_parts();

        self.method = method;
        self.uri = uri;
        self.version = version;
        self.headers = headers;
        self.extensions = extensions;
        self.body = body;
    }

    /// Returns a reference to the associated URI.
    ///
    /// # Examples
    ///
    /// ```
    /// # use salvo_core::http::*;
    /// let req = Request::default();
    /// assert_eq!(*req.uri(), *"/");
    /// ```
    #[inline]
    pub fn uri(&self) -> &Uri {
        &self.uri
    }

    /// Returns a mutable reference to the associated URI.
    ///
    /// *Notice: If you using this mutable reference to change the uri, you should change the `params` and `queries` manually.*
    ///
    /// # Examples
    ///
    /// ```
    /// # use salvo_core::http::*;
    /// let mut req: Request= Request::default();
    /// *req.uri_mut() = "/hello".parse().unwrap();
    /// assert_eq!(*req.uri(), *"/hello");
    /// ```
    #[inline]
    pub fn uri_mut(&mut self) -> &mut Uri {
        &mut self.uri
    }

    /// Set the associated URI. `querie` will be reset.
    ///
    /// *Notice: `params` will not reset.*
    #[inline]
    pub fn set_uri(&mut self, uri: Uri) {
        self.uri = uri;
        self.queries = OnceCell::new();
    }

    /// Returns a reference to the associated HTTP method.
    ///
    /// # Examples
    ///
    /// ```
    /// # use salvo_core::http::*;
    /// let req = Request::default();
    /// assert_eq!(*req.method(), Method::GET);
    /// ```
    #[inline]
    pub fn method(&self) -> &Method {
        &self.method
    }

    /// Returns a mutable reference to the associated HTTP method.
    ///
    /// # Examples
    ///
    /// ```
    /// # use salvo_core::http::*;
    /// let mut request: Request = Request::default();
    /// *request.method_mut() = Method::PUT;
    /// assert_eq!(*request.method(), Method::PUT);
    /// ```
    #[inline]
    pub fn method_mut(&mut self) -> &mut Method {
        &mut self.method
    }

    /// Returns the associated version.
    #[inline]
    pub fn version(&self) -> Version {
        self.version
    }
    /// Returns a mutable reference to the associated version.
    #[inline]
    pub fn version_mut(&mut self) -> &mut Version {
        &mut self.version
    }

    /// Returns the associated scheme.
    #[inline]
    pub fn scheme(&self) -> &Scheme {
        &self.scheme
    }
    /// Returns a mutable reference to the associated scheme.
    #[inline]
    pub fn scheme_mut(&mut self) -> &mut Scheme {
        &mut self.scheme
    }

    /// Get request remote address.
    #[inline]
    pub fn remote_addr(&self) -> &SocketAddr {
        &self.remote_addr
    }
    /// Get request remote address.
    #[inline]
    pub fn remote_addr_mut(&mut self) -> &mut SocketAddr {
        &mut self.remote_addr
    }

    /// Get request remote address reference.
    #[inline]
    pub fn local_addr(&self) -> &SocketAddr {
        &self.local_addr
    }
    /// Get mutable request remote address reference.
    #[inline]
    pub fn local_addr_mut(&mut self) -> &mut SocketAddr {
        &mut self.local_addr
    }

    /// Returns a reference to the associated header field map.
    ///
    /// # Examples
    ///
    /// ```
    /// # use salvo_core::http::*;
    /// let req = Request::default();
    /// assert!(req.headers().is_empty());
    /// ```
    #[inline]
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Returns a mutable reference to the associated header field map.
    ///
    /// # Examples
    ///
    /// ```
    /// # use salvo_core::http::*;
    /// # use salvo_core::http::header::*;
    /// let mut req: Request = Request::default();
    /// req.headers_mut().insert(HOST, HeaderValue::from_static("world"));
    /// assert!(!req.headers().is_empty());
    /// ```
    #[inline]
    pub fn headers_mut(&mut self) -> &mut HeaderMap<HeaderValue> {
        &mut self.headers
    }

    /// Get header with supplied name and try to parse to a 'T', returns None if failed or not found.
    #[inline]
    pub fn header<'de, T>(&'de self, key: impl AsHeaderName) -> Option<T>
    where
        T: Deserialize<'de>,
    {
        let values = self
            .headers
            .get_all(key)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .collect::<Vec<_>>();
        from_str_multi_val(values).ok()
    }

    /// Modify a header for this request.
    ///
    /// When `overwrite` is set to `true`, If the header is already present, the value will be replaced.
    /// When `overwrite` is set to `false`, The new header is always appended to the request, even if the header already exists.
    pub fn add_header<N, V>(&mut self, name: N, value: V, overwrite: bool) -> crate::Result<&mut Self>
    where
        N: IntoHeaderName,
        V: TryInto<HeaderValue>,
    {
        let value = value
            .try_into()
            .map_err(|_| Error::Other("invalid header value".into()))?;
        if overwrite {
            self.headers.insert(name, value);
        } else {
            self.headers.append(name, value);
        }
        Ok(self)
    }

    /// Returns a reference to the associated HTTP body.
    #[inline]
    pub fn body(&self) -> &ReqBody {
        &self.body
    }
    /// Returns a mutable reference to the associated HTTP body.
    #[inline]
    pub fn body_mut(&mut self) -> &mut ReqBody {
        &mut self.body
    }

    /// Sets body to a new value and returns old value.
    #[inline]
    pub fn replace_body(&mut self, body: ReqBody) -> ReqBody {
        std::mem::replace(&mut self.body, body)
    }

    /// Take body form the request, and set the body to None in the request.
    #[inline]
    pub fn take_body(&mut self) -> ReqBody {
        self.replace_body(ReqBody::None)
    }

    /// Returns a reference to the associated extensions.
    ///
    /// # Examples
    ///
    /// ```
    /// # use salvo_core::http::*;
    /// let req = Request::default();
    /// assert!(req.extensions().get::<i32>().is_none());
    /// ```
    #[inline]
    pub fn extensions(&self) -> &Extensions {
        &self.extensions
    }

    cfg_feature! {
        #![feature = "quinn"]

        #[inline]
        fn is_wt_connect(&self) -> bool {
            let protocol = self.extensions().get::<salvo_http3::ext::Protocol>();
            matches!((self.method(), protocol), (&Method::CONNECT, Some(p)) if p == &salvo_http3::ext::Protocol::WEB_TRANSPORT)
        }

        #[inline]
        /// Try to get a WebTransport session from the request.
        pub async fn web_transport_mut(&mut self) -> Result<&mut crate::proto::WebTransportSession<salvo_http3::http3_quinn::Connection, Bytes>, crate::Error> {
            if self.is_wt_connect() {
                if self.extensions.get::<crate::proto::WebTransportSession<salvo_http3::http3_quinn::Connection, Bytes>>().is_none() {
                    let conn = self.extensions.remove::<std::sync::Mutex<salvo_http3::server::Connection<salvo_http3::http3_quinn::Connection, Bytes>>>();
                    let stream = self.extensions.remove::<salvo_http3::server::RequestStream<salvo_http3::http3_quinn::BidiStream<Bytes>, Bytes>>();
                    if conn.is_some() && stream.is_some() {
                        let session =  crate::proto::WebTransportSession::accept(stream.unwrap(), conn.unwrap().into_inner().unwrap()).await?;
                        self.extensions.insert(session);
                        Ok(self.extensions.get_mut::<crate::proto::WebTransportSession<salvo_http3::http3_quinn::Connection, Bytes>>().unwrap())
                    } else {
                        if let Some(conn) = conn {
                            self.extensions_mut().insert(conn);
                        }
                        if let Some(stream) = stream {
                            self.extensions_mut().insert(stream);
                        }
                        Err(crate::Error::Other("invalid web transport".into()))
                    }
                } else {
                    Ok(self.extensions.get_mut::<crate::proto::WebTransportSession<salvo_http3::http3_quinn::Connection, Bytes>>().unwrap())
                }
            } else {
                Err(crate::Error::Other("no web transport".into()))
            }
        }
    }

    /// Returns a mutable reference to the associated extensions.
    ///
    /// # Examples
    ///
    /// ```
    /// # use salvo_core::http::*;
    /// # use salvo_core::http::header::*;
    /// let mut req = Request::default();
    /// req.extensions_mut().insert("hello");
    /// assert_eq!(req.extensions().get(), Some(&"hello"));
    /// ```
    #[inline]
    pub fn extensions_mut(&mut self) -> &mut Extensions {
        &mut self.extensions
    }

    /// Get accept.
    pub fn accept(&self) -> Vec<Mime> {
        let mut list: Vec<Mime> = vec![];
        if let Some(accept) = self.headers.get("accept").and_then(|h| h.to_str().ok()) {
            let parts: Vec<&str> = accept.split(',').collect();
            for part in parts {
                if let Ok(mt) = part.parse() {
                    list.push(mt);
                }
            }
        }
        list
    }

    /// Get first accept.
    #[inline]
    pub fn first_accept(&self) -> Option<Mime> {
        let mut accept = self.accept();
        if !accept.is_empty() {
            Some(accept.remove(0))
        } else {
            None
        }
    }

    /// Get content type.
    #[inline]
    pub fn content_type(&self) -> Option<Mime> {
        self.headers
            .get(CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            .and_then(|v| v.parse().ok())
    }

    cfg_feature! {
        #![feature = "cookie"]
        /// Get `CookieJar` reference.
        #[inline]
        pub fn cookies(&self) -> &CookieJar {
            &self.cookies
        }
        /// Get `CookieJar` mutable reference.
        #[inline]
        pub fn cookies_mut(&mut self) -> &mut CookieJar {
            &mut self.cookies
        }
        /// Get `Cookie` from cookies.
        #[inline]
        pub fn cookie<T>(&self, name: T) -> Option<&Cookie<'static>>
        where
            T: AsRef<str>,
        {
            self.cookies.get(name.as_ref())
        }
    }
    /// Get params reference.
    #[inline]
    pub fn params(&self) -> &IndexMap<String, String> {
        &self.params
    }
    /// Get params mutable reference.
    #[inline]
    pub fn params_mut(&mut self) -> &mut IndexMap<String, String> {
        &mut self.params
    }

    /// Get param value from params.
    #[inline]
    pub fn param<'de, T>(&'de self, key: &str) -> Option<T>
    where
        T: Deserialize<'de>,
    {
        self.params.get(key).and_then(|v| from_str_val(v).ok())
    }

    /// Get queries reference.
    pub fn queries(&self) -> &MultiMap<String, String> {
        self.queries.get_or_init(|| {
            form_urlencoded::parse(self.uri.query().unwrap_or_default().as_bytes())
                .into_owned()
                .collect()
        })
    }
    /// Get mutable queries reference.
    pub fn queries_mut(&mut self) -> &MultiMap<String, String> {
        let _ = self.queries();
        self.queries.get_mut().unwrap()
    }

    /// Get query value from queries.
    #[inline]
    pub fn query<'de, T>(&'de self, key: &str) -> Option<T>
    where
        T: Deserialize<'de>,
    {
        self.queries().get_vec(key).and_then(|vs| from_str_multi_val(vs).ok())
    }

    /// Get field data from form.
    #[inline]
    pub async fn form<'de, T>(&'de mut self, key: &str) -> Option<T>
    where
        T: Deserialize<'de>,
    {
        self.form_data()
            .await
            .ok()
            .and_then(|ps| ps.fields.get_vec(key))
            .and_then(|vs| from_str_multi_val(vs).ok())
    }

    /// Get field data from form, if key is not found in form data, then get from query.
    #[inline]
    pub async fn form_or_query<'de, T>(&'de mut self, key: &str) -> Option<T>
    where
        T: Deserialize<'de>,
    {
        if let Ok(form_data) = self.form_data().await {
            if form_data.fields.contains_key(key) {
                return self.form(key).await;
            }
        }
        self.query(key)
    }

    /// Get value from query, if key is not found in queries, then get from form.
    #[inline]
    pub async fn query_or_form<'de, T>(&'de mut self, key: &str) -> Option<T>
    where
        T: Deserialize<'de>,
    {
        if self.queries().contains_key(key) {
            self.query(key)
        } else {
            self.form(key).await
        }
    }

    /// Get [`FilePart`] reference from request.
    #[inline]
    pub async fn file<'a>(&'a mut self, key: &'a str) -> Option<&'a FilePart> {
        self.form_data().await.ok().and_then(|ps| ps.files.get(key))
    }

    /// Get [`FilePart`] reference from request.
    #[inline]
    pub async fn first_file(&mut self) -> Option<&FilePart> {
        self.form_data()
            .await
            .ok()
            .and_then(|ps| ps.files.iter().next())
            .map(|(_, f)| f)
    }

    /// Get [`FilePart`] list reference from request.
    #[inline]
    pub async fn files<'a>(&'a mut self, key: &'a str) -> Option<&'a Vec<FilePart>> {
        self.form_data().await.ok().and_then(|ps| ps.files.get_vec(key))
    }

    /// Get [`FilePart`] list reference from request.
    #[inline]
    pub async fn all_files(&mut self) -> Vec<&FilePart> {
        self.form_data()
            .await
            .ok()
            .map(|ps| ps.files.iter().map(|(_, f)| f).collect())
            .unwrap_or_default()
    }

    /// Get request payload with default max size limit(64KB).
    ///
    /// <https://github.com/hyperium/hyper/issues/3111>
    /// *Notice: This method takes body.
    #[inline]
    pub async fn payload(&mut self) -> Result<&Bytes, ParseError> {
        self.payload_with_max_size(secure_max_size()).await
    }

    /// Get request payload with max size limit.
    ///
    /// <https://github.com/hyperium/hyper/issues/3111>
    /// *Notice: This method takes body.
    #[inline]
    pub async fn payload_with_max_size(&mut self, max_size: usize) -> Result<&Bytes, ParseError> {
        let body = self.take_body();
        self.payload
            .get_or_try_init(|| async {
                Ok(Limited::new(body, max_size)
                    .collect()
                    .await
                    .map_err(ParseError::other)?
                    .to_bytes())
            })
            .await
    }

    /// Get `FormData` reference from request.
    ///
    /// *Notice: This method takes body and body's size is not limited.
    #[inline]
    pub async fn form_data(&mut self) -> Result<&FormData, ParseError> {
        if let Some(ctype) = self.content_type() {
            if ctype.subtype() == mime::WWW_FORM_URLENCODED || ctype.type_() == mime::MULTIPART {
                let body = self.take_body();
                let headers = self.headers();
                self.form_data
                    .get_or_try_init(|| async { FormData::read(headers, body).await })
                    .await
            } else {
                Err(ParseError::NotFormData)
            }
        } else {
            Err(ParseError::NotFormData)
        }
    }

    /// Extract request as type `T` from request's different parts.
    #[inline]
    pub async fn extract<'de, T>(&'de mut self) -> Result<T, ParseError>
    where
        T: Extractible<'de> + Send,
    {
        self.extract_with_metadata(T::metadata()).await
    }

    /// Extract request as type `T` from request's different parts.
    #[inline]
    pub async fn extract_with_metadata<'de, T>(&'de mut self, metadata: &'de Metadata) -> Result<T, ParseError>
    where
        T: Deserialize<'de> + Send,
    {
        from_request(self, metadata).await
    }

    /// Parse url params as type `T` from request.
    #[inline]
    pub fn parse_params<'de, T>(&'de mut self) -> Result<T, ParseError>
    where
        T: Deserialize<'de>,
    {
        let params = self.params().iter();
        from_str_map(params).map_err(ParseError::Deserialize)
    }

    /// Parse queries as type `T` from request.
    #[inline]
    pub fn parse_queries<'de, T>(&'de mut self) -> Result<T, ParseError>
    where
        T: Deserialize<'de>,
    {
        let queries = self.queries().iter_all();
        from_str_multi_map(queries).map_err(ParseError::Deserialize)
    }

    /// Parse headers as type `T` from request.
    #[inline]
    pub fn parse_headers<'de, T>(&'de mut self) -> Result<T, ParseError>
    where
        T: Deserialize<'de>,
    {
        let iter = self
            .headers()
            .iter()
            .map(|(k, v)| (k.as_str(), v.to_str().unwrap_or_default()));
        from_str_map(iter).map_err(ParseError::Deserialize)
    }

    cfg_feature! {
        #![feature = "cookie"]
        /// Parse cookies as type `T` from request.
        #[inline]
        pub fn parse_cookies<'de, T>(&'de mut self) -> Result<T, ParseError>
        where
            T: Deserialize<'de>,
        {
            let iter = self
                .cookies()
                .iter()
                .map(|c| c.name_value());
            from_str_map(iter).map_err(ParseError::Deserialize)
        }
    }

    /// Parse json body as type `T` from request with default max size limit.
    #[inline]
    pub async fn parse_json<'de, T>(&'de mut self) -> Result<T, ParseError>
    where
        T: Deserialize<'de>,
    {
        self.parse_json_with_max_size(secure_max_size()).await
    }
    /// Parse json body as type `T` from request with max size limit.
    #[inline]
    pub async fn parse_json_with_max_size<'de, T>(&'de mut self, max_size: usize) -> Result<T, ParseError>
    where
        T: Deserialize<'de>,
    {
        let ctype = self.content_type();
        if let Some(ctype) = ctype {
            if ctype.subtype() == mime::JSON {
                return self
                    .payload_with_max_size(max_size)
                    .await
                    .and_then(|payload| serde_json::from_slice::<T>(payload).map_err(ParseError::SerdeJson));
            }
        }
        Err(ParseError::InvalidContentType)
    }

    /// Parse form body as type `T` from request.
    #[inline]
    pub async fn parse_form<'de, T>(&'de mut self) -> Result<T, ParseError>
    where
        T: Deserialize<'de>,
    {
        if let Some(ctype) = self.content_type() {
            if ctype.subtype() == mime::WWW_FORM_URLENCODED || ctype.subtype() == mime::FORM_DATA {
                return from_str_multi_map(self.form_data().await?.fields.iter_all()).map_err(ParseError::Deserialize);
            }
        }
        Err(ParseError::InvalidContentType)
    }

    /// Parse json body or form body as type `T` from request with default max size.
    #[inline]
    pub async fn parse_body<'de, T>(&'de mut self) -> Result<T, ParseError>
    where
        T: Deserialize<'de>,
    {
        self.parse_body_with_max_size(secure_max_size()).await
    }

    /// Parse json body or form body as type `T` from request with max size.
    #[inline]
    pub async fn parse_body_with_max_size<'de, T>(&'de mut self, max_size: usize) -> Result<T, ParseError>
    where
        T: Deserialize<'de>,
    {
        if let Some(ctype) = self.content_type() {
            if ctype.subtype() == mime::WWW_FORM_URLENCODED || ctype.subtype() == mime::FORM_DATA {
                return from_str_multi_map(self.form_data().await?.fields.iter_all()).map_err(ParseError::Deserialize);
            } else if ctype.subtype() == mime::JSON {
                return self
                    .payload_with_max_size(max_size)
                    .await
                    .and_then(|body| serde_json::from_slice::<T>(body).map_err(ParseError::SerdeJson));
            }
        }
        Err(ParseError::InvalidContentType)
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::test::TestClient;

    #[tokio::test]
    async fn test_parse_queries() {
        #[derive(Deserialize, Eq, PartialEq, Debug)]
        struct BadMan<'a> {
            name: &'a str,
            age: u8,
            wives: Vec<String>,
            weapons: (u64, String, String),
        }
        #[derive(Deserialize, Eq, PartialEq, Debug)]
        struct GoodMan {
            name: String,
            age: u8,
            wives: String,
            weapons: u64,
        }
        let mut req = TestClient::get(
            "http://127.0.0.1:5801/hello?name=rust&age=25&wives=a&wives=2&weapons=69&weapons=stick&weapons=gun",
        )
        .build();
        let man = req.parse_queries::<BadMan>().unwrap();
        assert_eq!(man.name, "rust");
        assert_eq!(man.age, 25);
        assert_eq!(man.wives, vec!["a", "2"]);
        assert_eq!(man.weapons, (69, "stick".into(), "gun".into()));
        let man = req.parse_queries::<GoodMan>().unwrap();
        assert_eq!(man.name, "rust");
        assert_eq!(man.age, 25);
        assert_eq!(man.wives, "a");
        assert_eq!(man.weapons, 69);
    }

    #[tokio::test]
    async fn test_parse_json() {
        #[derive(Serialize, Deserialize, Eq, PartialEq, Debug)]
        struct User {
            name: String,
        }
        let mut req = TestClient::get("http://127.0.0.1:5800/hello")
            .json(&User { name: "jobs".into() })
            .build();
        assert_eq!(req.parse_json::<User>().await.unwrap(), User { name: "jobs".into() });
    }
    #[tokio::test]
    async fn test_query() {
        let req = TestClient::get("http://127.0.0.1:5801/hello?name=rust&name=25&name=a&name=2&weapons=98&weapons=gun")
            .build();
        assert_eq!(req.queries().len(), 2);
        assert_eq!(req.query::<String>("name").unwrap(), "rust");
        assert_eq!(req.query::<&str>("name").unwrap(), "rust");
        assert_eq!(req.query::<i64>("weapons").unwrap(), 98);
        let names = req.query::<Vec<&str>>("name").unwrap();
        let weapons = req.query::<(u64, &str)>("weapons").unwrap();
        assert_eq!(names, vec!["rust", "25", "a", "2"]);
        assert_eq!(weapons, (98, "gun"));
    }
    #[tokio::test]
    async fn test_form() {
        let mut req = TestClient::post("http://127.0.0.1:5800/hello?q=rust")
            .add_header("content-type", "application/x-www-form-urlencoded", true)
            .raw_form("lover=dog&money=sh*t&q=firefox")
            .build();
        assert_eq!(req.form::<String>("money").await.unwrap(), "sh*t");
        assert_eq!(req.query_or_form::<String>("q").await.unwrap(), "rust");
        assert_eq!(req.form_or_query::<String>("q").await.unwrap(), "firefox");

        let mut req: Request = TestClient::post("http://127.0.0.1:5800/hello?q=rust")
            .add_header(
                "content-type",
                "multipart/form-data; boundary=----WebKitFormBoundary0mkL0yrNNupCojyz",
                true,
            )
            .body(
                "------WebKitFormBoundary0mkL0yrNNupCojyz\r\n\
Content-Disposition: form-data; name=\"money\"\r\n\r\nsh*t\r\n\
------WebKitFormBoundary0mkL0yrNNupCojyz\r\n\
Content-Disposition: form-data; name=\"file1\"; filename=\"err.txt\"\r\n\
Content-Type: text/plain\r\n\r\n\
file content\r\n\
------WebKitFormBoundary0mkL0yrNNupCojyz--\r\n",
            )
            .build();
        assert_eq!(req.form::<String>("money").await.unwrap(), "sh*t");
        let file = req.file("file1").await.unwrap();
        assert_eq!(file.name().unwrap(), "err.txt");
        assert_eq!(file.headers().get("content-type").unwrap(), "text/plain");
        let files = req.files("file1").await.unwrap();
        assert_eq!(files[0].name().unwrap(), "err.txt");
    }
}
