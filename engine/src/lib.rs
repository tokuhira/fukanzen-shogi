pub mod board;
pub mod kifu;
pub mod movegen;
pub mod resolve;
pub mod serialize;
pub mod terminate;
pub mod types;

/// このクレートが実装するルール仕様の版。(major, minor)
/// ルール v0.5 の挙動を実装している。
pub const RULE_VERSION: (u32, u32) = (0, 5);
