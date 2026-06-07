//! Rust port of PostgreSQL's pglz compressor (`src/common/pg_lzcompress.c`).
//!
//! Provides synchronous compression and decompression over byte slices,
//! plus an optional async wrapper (`tokio` feature) over `AsyncRead` /
//! `AsyncWrite`.

mod compress;
mod decompress;
mod strategy;

pub use compress::{compress, compress_into, max_compressed_buffer_size};
pub use decompress::{
    decompress, decompress_into, decompress_with_capacity, maximum_compressed_size,
};
pub use strategy::Strategy;

#[cfg(feature = "tokio")]
pub mod tokio_io;
