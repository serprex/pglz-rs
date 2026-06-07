Rust port of PostgreSQL's pglz compressor (`src/common/pg_lzcompress.c`).

## Sync

```rust
use pglz::{compress, decompress, Strategy};

let data = b"some bytes to compress, repeated, repeated, repeated";
let compressed = compress(data, &Strategy::ALWAYS).unwrap();
let roundtrip = decompress(&compressed).unwrap();
assert_eq!(roundtrip, data);
```

Caller-owned buffers and capacity hints:

```rust
use pglz::{
    compress_into, decompress_into, decompress_with_capacity, max_compressed_buffer_size, Strategy,
};

let mut buf = vec![0u8; max_compressed_buffer_size(data.len())];
let n = compress_into(data, &mut buf, &Strategy::DEFAULT).unwrap();

let mut out = vec![0u8; data.len()];
decompress_into(&buf[..n], &mut out, true).unwrap();

let out = decompress_with_capacity(&buf[..n], data.len()).unwrap();
```

## Strategy

`Strategy::DEFAULT` mirrors PostgreSQL's TOAST default. `Strategy::ALWAYS` skips ratio/size gating. Fields are public for custom tuning.

## Async (tokio)

Enable `tokio` feature:

```toml
pglz = { version = "0.1", features = ["tokio"] }
```

```rust
use pglz::{tokio_io, Strategy};

tokio_io::compress(&mut reader, &mut writer, &Strategy::DEFAULT).await?;
tokio_io::decompress(&mut reader, &mut writer).await?;
```

Whole input is buffered in memory; pglz needs random access to find back-references.

## Benchmark

```sh
cargo run --release --example decompress_bench
```
