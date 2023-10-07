//! Http body.
pub use hyper::body::{Body, SizeHint, Frame};

mod req;
#[cfg(feature = "quinn")]
pub use req::h3::H3ReqBody;
pub use req::ReqBody;
mod res;
pub use hyper::body::Incoming as HyperBody;
pub use res::ResBody;
mod channel;
pub use channel::{BodySender, BodyReceiver};
