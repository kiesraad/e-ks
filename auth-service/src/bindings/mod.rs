//! SAML transport bindings (eID §3.1.1): the browser-facing HTTP-POST binding
//! and the mTLS SOAP back-channel.

pub mod http_post;
pub mod mtls;
pub mod soap;
