//! Public URLs of the bundled frontend assets.
//!
//! In release builds memory-serve serves every bundled file on a hashed,
//! immutable route as well, and templates reference assets through its
//! manifest so the URL changes whenever the file does. In development the
//! esbuild dev server is proxied and the plain route is used; the URLs differ
//! but templates never need to know which mode is active.

/// Prefix under which the asset bundle is mounted.
pub const STATIC_PREFIX: &str = "/static";

/// The embedded asset bundle, configured once so the router and the manifest
/// agree on the routes.
#[cfg(feature = "memory-serve")]
pub(crate) fn memory_serve() -> memory_serve::MemoryServe {
    memory_serve::load!()
        .index_file(None)
        .enable_hashed_routes(true)
}

#[cfg(feature = "memory-serve")]
static MANIFEST: std::sync::LazyLock<memory_serve::Manifest> =
    std::sync::LazyLock::new(|| memory_serve().manifest());

/// Public URL of the bundled asset at `route`, e.g. `/index.css`.
pub fn asset_path(route: &str) -> String {
    #[cfg(feature = "memory-serve")]
    let route = MANIFEST.get(route).unwrap_or(route);

    format!("{STATIC_PREFIX}{route}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_path_is_mounted_under_static() {
        let path = asset_path("/index.css");

        assert!(path.starts_with("/static/index."), "{path}");
        assert!(path.ends_with(".css"), "{path}");
    }

    #[cfg(feature = "memory-serve")]
    #[test]
    fn bundled_assets_get_a_hashed_route() {
        assert_ne!(asset_path("/index.css"), "/static/index.css");
        assert_ne!(asset_path("/index.js"), "/static/index.js");
    }

    #[test]
    fn unknown_assets_keep_their_plain_route() {
        assert_eq!(asset_path("/missing.css"), "/static/missing.css");
    }
}
