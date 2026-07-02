pub mod archive;
pub mod board;
pub mod kifu;
pub mod movegen;
pub mod resolve;
pub mod serialize;
pub mod terminate;
pub mod types;

/// このクレートが実装するルール仕様の版。(major, minor)
/// ルール v0.6 の挙動を実装している（引き分けの正式化・千日手確定・最長手数500組手）。
pub const RULE_VERSION: (u32, u32) = (0, 6);
