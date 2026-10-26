//! Translations: the template every spoken line is matched against, the
//! language files that carry each translation, and the checks a translated
//! line must pass.

pub mod check;
pub mod extract;
mod po;
mod rust_strings;
