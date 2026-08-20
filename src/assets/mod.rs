//! Icons and avatars.
//!
//! Both return a URL, a source, and the alternatives. Neither ever puts image
//! bytes into an MCP message: a base64 PNG in a tool result costs the caller
//! thousands of tokens and tells it nothing it could not get from the URL.

#[cfg(feature = "avatars")]
pub mod avatars;
#[cfg(feature = "icons")]
pub mod icons;
