use engine::board::Position;
use engine::serialize::canonical_bytes;
use sha2::{Digest, Sha256};

/// 盤面正準直列化の SHA-256（盤面＋持ち駒＋手数）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoardHash(pub [u8; 32]);

pub fn board_hash(pos: &Position) -> BoardHash {
    let bytes = canonical_bytes(pos);
    let mut h = Sha256::new();
    h.update(&bytes);
    BoardHash(h.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_position_same_hash() {
        let pos = Position::initial();
        assert_eq!(board_hash(&pos), board_hash(&pos));
    }

    /// 盤面ハッシュ相互検証: 手数が異なれば異なるハッシュ（正準直列化が手数を含む）
    #[test]
    fn different_move_number_different_hash() {
        let pos1 = Position::initial();
        let mut pos2 = Position::initial();
        pos2.move_number = 99;
        assert_ne!(board_hash(&pos1).0, board_hash(&pos2).0);
    }
}
