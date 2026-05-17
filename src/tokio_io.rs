//! Async compression/decompression over tokio's `AsyncRead` / `AsyncWrite`.
//!
//! pglz needs the whole input buffered before it can find back-references,
//! so these wrappers read everything to memory, run the synchronous core,
//! then write the result.

use crate::Strategy;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Read `reader` to end, compress with `strategy`, write the result to
/// `writer`. Returns the number of bytes written, or `None` if compression
/// failed.
pub async fn compress<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    mut reader: R,
    mut writer: W,
    strategy: &Strategy,
) -> std::io::Result<Option<usize>> {
    let mut input = Vec::new();
    reader.read_to_end(&mut input).await?;
    let Some(out) = crate::compress(&input, strategy) else {
        return Ok(None);
    };
    writer.write_all(&out).await?;
    Ok(Some(out.len()))
}

/// Read `reader` to end, decompress, write the result to `writer`. Returns
/// the number of decompressed bytes, or `None` if the compressed stream is
/// corrupted.
pub async fn decompress<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    mut reader: R,
    mut writer: W,
) -> std::io::Result<Option<usize>> {
    let mut input = Vec::new();
    reader.read_to_end(&mut input).await?;
    let Some(out) = crate::decompress(&input) else {
        return Ok(None);
    };
    writer.write_all(&out).await?;
    Ok(Some(out.len()))
}
