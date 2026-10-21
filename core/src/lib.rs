//! speeddemon-core — zero-dependency shared library
//!
//! Every module is std-only (D=1). Unsafe is forbidden at the crate level.
//! At H0 this ships as a thin correctness skeleton; the deep optimization
//! (SIMD intrinsics, per-track specialization) replaces the scalar fallbacks.

pub mod arena;
pub mod buf_reader;
pub mod det_writer;
pub mod fixed_codec;
pub mod pool;
pub mod simd_scan;
