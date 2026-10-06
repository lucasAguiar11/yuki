//! Re-exports of the cookie domain model. The canonical types live in
//! `yuki-core` so `HttpResponse.captured_cookies` and the storage layer can
//! reference them without depending on this crate.
pub use yuki_core::{CookieJar, SameSite, StoredCookie};
