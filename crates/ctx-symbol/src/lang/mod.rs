//! Language backends. Add a new backend module here and register it in
//! `crate::language::REGISTRY`.

pub mod go;
pub mod java;
pub mod javascript;
pub mod python;
pub mod rust;
pub mod typescript;
