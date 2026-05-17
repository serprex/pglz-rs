//! Compression strategy controlling pglz behavior. Mirrors `PGLZ_Strategy`
//! from `src/include/common/pg_lzcompress.h` in PostgreSQL.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Strategy {
    /// Minimum input data size to consider compression.
    pub min_input_size: i32,
    /// Maximum input data size to consider compression.
    pub max_input_size: i32,
    /// Minimum compression rate (0-99%) to require.
    pub min_comp_rate: i32,
    /// Abandon compression if no compressible data within first N bytes.
    pub first_success_by: i32,
    /// Stop history lookup once a match of this many bytes is found.
    pub match_size_good: i32,
    /// Lower match_size_good by this percentage at every loop iteration.
    pub match_size_drop: i32,
}

impl Strategy {
    /// Recommended default strategy for TOAST. Mirrors `PGLZ_strategy_default`.
    pub const DEFAULT: Strategy = Strategy {
        min_input_size: 32,
        max_input_size: i32::MAX,
        min_comp_rate: 25,
        first_success_by: 1024,
        match_size_good: 128,
        match_size_drop: 10,
    };

    /// Try to compress inputs of any length. Mirrors `PGLZ_strategy_always`.
    pub const ALWAYS: Strategy = Strategy {
        min_input_size: 0,
        max_input_size: i32::MAX,
        min_comp_rate: 0,
        first_success_by: i32::MAX,
        match_size_good: 128,
        match_size_drop: 6,
    };
}

impl Default for Strategy {
    fn default() -> Self {
        Strategy::DEFAULT
    }
}
