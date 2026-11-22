//! Translations: the template every spoken line is matched against, the
//! language files that carry each translation, and the checks a translated
//! line must pass.

pub mod check;
#[cfg(feature = "world")]
pub mod diff;
pub mod extract;
mod po;
mod rust_strings;
