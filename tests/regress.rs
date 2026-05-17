//! Tests ported from PostgreSQL's `src/test/regress/sql/compression_pglz.sql`.
//! The original SQL test drives `test_pglz_compress` / `test_pglz_decompress`
//! defined in `src/test/regress/regress.c`, which call `pglz_compress` with
//! the `always` strategy and `pglz_decompress` with the rawsize/check_complete
//! parameters.

use pglz::{Strategy, compress, decompress, decompress_into};

fn abcd_400() -> Vec<u8> {
    "abcd".repeat(100).into_bytes()
}

#[test]
fn roundtrip_via_decompress() {
    let input = abcd_400();
    let compressed = compress(&input, &Strategy::ALWAYS).expect("compress");
    let out = decompress(&compressed).expect("decompress");
    assert_eq!(out, input);
}

#[test]
fn roundtrip_via_decompress_into_check_complete() {
    let input = abcd_400();
    let compressed = compress(&input, &Strategy::ALWAYS).expect("compress");
    let mut buf = vec![0u8; 400];
    let n = decompress_into(&compressed, &mut buf, true).expect("decompress");
    assert_eq!(n, 400);
    assert_eq!(buf, input);
}

#[test]
fn decompress_into_dest_too_large_fails_check_complete() {
    let input = abcd_400();
    let compressed = compress(&input, &Strategy::ALWAYS).expect("compress");
    let mut buf = vec![0u8; 500];
    assert!(decompress_into(&compressed, &mut buf, true).is_none());
}

#[test]
fn decompress_into_dest_too_small_fails_check_complete() {
    let input = abcd_400();
    let compressed = compress(&input, &Strategy::ALWAYS).expect("compress");
    let mut buf = vec![0u8; 100];
    assert!(decompress_into(&compressed, &mut buf, true).is_none());
}

// Corrupted: control byte sets tag flag but no data follows.
#[test]
fn corrupt_ctrl_only_no_check() {
    let mut buf = vec![0u8; 1024];
    let n = decompress_into(&[0x01], &mut buf, false).expect("decompress");
    assert_eq!(n, 0);
}

#[test]
fn corrupt_ctrl_only_check() {
    let mut buf = vec![0u8; 1024];
    assert!(decompress_into(&[0x01], &mut buf, true).is_none());
}

// Stray control byte alone decompresses to an empty stream via decompress.
#[test]
fn ctrl_only_full_decompress_is_empty() {
    let v = decompress(&[0x01]).expect("decompress");
    assert!(v.is_empty());
}

// Corrupted: control byte sets tag flag, only 1 byte of the 2-byte tag follows.
#[test]
fn corrupt_partial_tag() {
    let mut buf = vec![0u8; 1024];
    assert!(decompress_into(&[0x01, 0xff], &mut buf, false).is_none());
    assert!(decompress_into(&[0x01, 0xff], &mut buf, true).is_none());
    assert!(decompress(&[0x01, 0xff]).is_none());
}

// Corrupted: extended-length tag (nibble 0x0f) without third byte.
#[test]
fn corrupt_extended_tag() {
    let mut buf = vec![0u8; 1024];
    assert!(decompress_into(&[0x01, 0x0f, 0x01], &mut buf, false).is_none());
    assert!(decompress_into(&[0x01, 0x0f, 0x01], &mut buf, true).is_none());
    assert!(decompress(&[0x01, 0x0f, 0x01]).is_none());
}

// Extra coverage beyond the SQL test: a few inputs of varying shapes.

#[test]
fn roundtrip_default_strategy_long_text() {
    let input: Vec<u8> = "The quick brown fox jumps over the lazy dog. "
        .repeat(50)
        .into_bytes();
    let compressed = compress(&input, &Strategy::DEFAULT).expect("compress");
    assert!(compressed.len() < input.len());
    assert_eq!(decompress(&compressed).expect("decompress"), input);
}

#[test]
fn default_strategy_rejects_small_inputs() {
    // DEFAULT.min_input_size = 32, so anything shorter is refused.
    assert!(compress(b"short", &Strategy::DEFAULT).is_none());
}

#[test]
fn always_strategy_handles_single_byte_repetition() {
    let input = vec![b'a'; 1024];
    let compressed = compress(&input, &Strategy::ALWAYS).expect("compress");
    assert!(compressed.len() < input.len());
    assert_eq!(decompress(&compressed).expect("decompress"), input);
}

#[test]
fn always_strategy_max_match_length() {
    // 300 bytes of 'x' exercises the extended-length tag path (len > 17).
    let input = vec![b'x'; 300];
    let compressed = compress(&input, &Strategy::ALWAYS).expect("compress");
    assert_eq!(decompress(&compressed).expect("decompress"), input);
}

#[cfg(feature = "tokio")]
mod async_tests {
    use pglz::{Strategy, tokio_io};

    #[tokio::test]
    async fn tokio_roundtrip() {
        let input = "abcd".repeat(100).into_bytes();
        let mut compressed = Vec::new();
        tokio_io::compress(&input[..], &mut compressed, &Strategy::ALWAYS)
            .await
            .expect("io")
            .expect("compress");
        let mut decompressed = Vec::new();
        tokio_io::decompress(&compressed[..], &mut decompressed)
            .await
            .expect("io")
            .expect("decompress");
        assert_eq!(decompressed, input);
    }
}
