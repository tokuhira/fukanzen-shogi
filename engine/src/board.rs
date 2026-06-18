use crate::types::{Piece, PieceKind, Side, Square};

/// 9×9 盤面。インデックス = (file-1)*9 + (rank-1)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Board {
    squares: [Option<Piece>; 81],
}

impl Board {
    pub fn empty() -> Self {
        Self {
            squares: [None; 81],
        }
    }

    pub fn get(&self, sq: Square) -> Option<Piece> {
        self.squares[sq.index() as usize]
    }

    pub fn set(&mut self, sq: Square, piece: Option<Piece>) {
        self.squares[sq.index() as usize] = piece;
    }

    pub fn iter(&self) -> impl Iterator<Item = (Square, Piece)> + '_ {
        self.squares.iter().enumerate().filter_map(|(i, p)| {
            p.map(|piece| (Square::from_index(i as u8), piece))
        })
    }
}

/// 持ち駒。打てる駒種（歩・香・桂・銀・金・角・飛）ごとの枚数。玉は持ち駒にならない。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Hand {
    counts: [u8; 7],
}

/// 持ち駒として使える駒種の順序インデックス
const HAND_KINDS: [PieceKind; 7] = [
    PieceKind::Pawn,
    PieceKind::Lance,
    PieceKind::Knight,
    PieceKind::Silver,
    PieceKind::Gold,
    PieceKind::Bishop,
    PieceKind::Rook,
];

fn hand_index(kind: PieceKind) -> Option<usize> {
    HAND_KINDS.iter().position(|&k| k == kind)
}

fn hand_index_expect(kind: PieceKind) -> usize {
    hand_index(kind).unwrap_or_else(|| panic!("invalid hand piece kind: {:?}", kind))
}

impl Hand {
    pub fn empty() -> Self {
        Self { counts: [0; 7] }
    }

    /// 持ち駒の枚数を返す。HAND_KINDS に含まれない種（玉・成駒）は 0 を返す。
    pub fn count(&self, kind: PieceKind) -> u8 {
        hand_index(kind).map_or(0, |i| self.counts[i])
    }

    pub fn add(&mut self, kind: PieceKind) {
        let idx = hand_index_expect(kind);
        self.counts[idx] += 1;
    }

    pub fn remove(&mut self, kind: PieceKind) {
        let idx = hand_index_expect(kind);
        debug_assert!(self.counts[idx] > 0, "tried to remove piece not in hand");
        self.counts[idx] -= 1;
    }

    pub fn has(&self, kind: PieceKind) -> bool {
        self.count(kind) > 0
    }

    pub fn kinds() -> &'static [PieceKind] {
        &HAND_KINDS
    }

    pub fn iter(&self) -> impl Iterator<Item = (PieceKind, u8)> + '_ {
        HAND_KINDS
            .iter()
            .copied()
            .zip(self.counts.iter().copied())
            .filter(|&(_, cnt)| cnt > 0)
    }
}

/// 局面（ハッシュ可能状態）。
/// 盤面＋双方の持ち駒＋手数 の三要素が、第二段階の盤面ハッシュ・中断救済の基盤となる。
/// 手番フィールドは持たない（同時着手のため存在しない）。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Position {
    pub board: Board,
    pub hand_sente: Hand,
    pub hand_gote: Hand,
    /// 何ターン目か（1始まり）
    pub move_number: u32,
}

impl Position {
    pub fn hand(&self, side: Side) -> &Hand {
        match side {
            Side::Sente => &self.hand_sente,
            Side::Gote => &self.hand_gote,
        }
    }

    pub fn hand_mut(&mut self, side: Side) -> &mut Hand {
        match side {
            Side::Sente => &mut self.hand_sente,
            Side::Gote => &mut self.hand_gote,
        }
    }

    /// 千日手判定用のコンテンツキー（手数を除いた盤面＋持ち駒のみ）
    pub fn content_key(&self) -> (Board, Hand, Hand) {
        (
            self.board.clone(),
            self.hand_sente.clone(),
            self.hand_gote.clone(),
        )
    }

    /// 初期局面を生成する
    pub fn initial() -> Self {
        let mut board = Board::empty();

        // 後手（上手前、段1–3）の初期配置
        // 段1: 香桂銀金玉金銀桂香（ファイル9から1）
        let back_order = [
            PieceKind::Lance,
            PieceKind::Knight,
            PieceKind::Silver,
            PieceKind::Gold,
            PieceKind::King,
            PieceKind::Gold,
            PieceKind::Silver,
            PieceKind::Knight,
            PieceKind::Lance,
        ];
        for (i, &kind) in back_order.iter().enumerate() {
            let file = 9 - i as u8; // 9,8,7,...,1
            board.set(
                Square::new(file, 1),
                Some(Piece::new(kind, Side::Gote)),
            );
        }
        // 段2: 飛（ファイル2）、角（ファイル8）
        board.set(Square::new(2, 2), Some(Piece::new(PieceKind::Rook, Side::Gote)));
        board.set(Square::new(8, 2), Some(Piece::new(PieceKind::Bishop, Side::Gote)));
        // 段3: 歩 全筋
        for file in 1u8..=9 {
            board.set(
                Square::new(file, 3),
                Some(Piece::new(PieceKind::Pawn, Side::Gote)),
            );
        }

        // 先手（下手前、段7–9）の初期配置
        // 段9: 香桂銀金玉金銀桂香（ファイル1から9）
        let back_order_sente = [
            PieceKind::Lance,
            PieceKind::Knight,
            PieceKind::Silver,
            PieceKind::Gold,
            PieceKind::King,
            PieceKind::Gold,
            PieceKind::Silver,
            PieceKind::Knight,
            PieceKind::Lance,
        ];
        for (i, &kind) in back_order_sente.iter().enumerate() {
            let file = i as u8 + 1; // 1,2,3,...,9
            board.set(
                Square::new(file, 9),
                Some(Piece::new(kind, Side::Sente)),
            );
        }
        // 段8: 角（ファイル2）、飛（ファイル8）
        board.set(Square::new(2, 8), Some(Piece::new(PieceKind::Bishop, Side::Sente)));
        board.set(Square::new(8, 8), Some(Piece::new(PieceKind::Rook, Side::Sente)));
        // 段7: 歩 全筋
        for file in 1u8..=9 {
            board.set(
                Square::new(file, 7),
                Some(Piece::new(PieceKind::Pawn, Side::Sente)),
            );
        }

        Position {
            board,
            hand_sente: Hand::empty(),
            hand_gote: Hand::empty(),
            move_number: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_position_pieces() {
        let pos = Position::initial();
        // 先手玉
        assert_eq!(
            pos.board.get(Square::new(5, 9)),
            Some(Piece::new(PieceKind::King, Side::Sente))
        );
        // 後手玉
        assert_eq!(
            pos.board.get(Square::new(5, 1)),
            Some(Piece::new(PieceKind::King, Side::Gote))
        );
        // 先手歩
        for file in 1u8..=9 {
            assert_eq!(
                pos.board.get(Square::new(file, 7)),
                Some(Piece::new(PieceKind::Pawn, Side::Sente))
            );
        }
        // 後手歩
        for file in 1u8..=9 {
            assert_eq!(
                pos.board.get(Square::new(file, 3)),
                Some(Piece::new(PieceKind::Pawn, Side::Gote))
            );
        }
    }

    #[test]
    fn hand_add_remove() {
        let mut hand = Hand::empty();
        hand.add(PieceKind::Pawn);
        hand.add(PieceKind::Pawn);
        assert_eq!(hand.count(PieceKind::Pawn), 2);
        hand.remove(PieceKind::Pawn);
        assert_eq!(hand.count(PieceKind::Pawn), 1);
    }
}
