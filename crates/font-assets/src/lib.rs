//! Shared font storage, without a font database, fallback policy or UI dependency.
//!
//! Editors keep their existing font selection and shaping. Each approved font is
//! embedded into one named static; consumers borrow it rather than embedding it
//! again. The application remains independent of build-input paths at runtime.
#![forbid(unsafe_code)]

include!(concat!(env!("OUT_DIR"), "/assets.rs"));
