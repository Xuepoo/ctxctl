//! Language backends. Add a new backend module here and register it in
//! `crate::language::REGISTRY`.

pub mod c;
pub mod cpp;
pub mod csharp;
pub mod css;
pub mod go;
pub mod html;
pub mod java;
pub mod javascript;
pub mod lua;
pub mod markdown;
pub mod python;
pub mod ruby;
pub mod rust;
pub mod typescript;
pub(crate) mod util;
