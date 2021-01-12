pub use salvo_core as core;
pub use salvo_core::*;

#[cfg(feature = "macros")]
pub use salvo_macros;

#[cfg(feature = "extra")]
pub use salvo_extra as extra;

pub mod prelude {
    pub use crate::depot::Depot;
    pub use crate::http::{Request, Response};
    pub use crate::routing::filter;
    pub use crate::routing::Router;
    pub use crate::server::{Server, ServerConfig};
    pub use crate::writer::*;
    pub use crate::{fn_handler, fn_one_handler, FnHandler, Handler};
    pub use async_trait::async_trait;
    #[cfg(feature = "macros")]
    pub use salvo_macros::fn_handler;
}
