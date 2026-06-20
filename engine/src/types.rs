/// 陣営。"先手/後手" は手番順ではなく単なる陣営ラベル（同時着手のため手番は存在しない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Sente,
    Gote,
}

impl Side {
    pub fn opposite(self) -> Self {
        match self {
            Side::Sente => Side::Gote,
            Side::Gote => Side::Sente,
        }
    }
}

/// 駒種。コンパイラの網羅性検査を活かすため全14種を直接列挙する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PieceKind {
    // 基本8種
    Pawn,   // 歩 P
    Lance,  // 香 L
    Knight, // 桂 N
    Silver, // 銀 S
    Gold,   // 金 G
    Bishop, // 角 B
    Rook,   // 飛 R
    King,   // 玉 K
    // 成駒6種
    ProPawn,   // と金 +P
    ProLance,  // 成香 +L
    ProKnight, // 成桂 +N
    ProSilver, // 成銀 +S
    Horse,     // 馬  +B
    Dragon,    // 龍  +R
}

impl PieceKind {
    /// 成れるか（金・玉は成れない）
    pub fn can_promote(self) -> bool {
        matches!(
            self,
            PieceKind::Pawn
                | PieceKind::Lance
                | PieceKind::Knight
                | PieceKind::Silver
                | PieceKind::Bishop
                | PieceKind::Rook
        )
    }

    /// 成った駒種を返す（成れない種への呼び出しはパニック）
    pub fn promoted(self) -> Self {
        match self {
            PieceKind::Pawn => PieceKind::ProPawn,
            PieceKind::Lance => PieceKind::ProLance,
            PieceKind::Knight => PieceKind::ProKnight,
            PieceKind::Silver => PieceKind::ProSilver,
            PieceKind::Bishop => PieceKind::Horse,
            PieceKind::Rook => PieceKind::Dragon,
            _ => panic!("cannot promote {:?}", self),
        }
    }

    /// 持ち駒にするとき成りを解除して基本種へ戻す
    pub fn unpromoted(self) -> Self {
        match self {
            PieceKind::ProPawn => PieceKind::Pawn,
            PieceKind::ProLance => PieceKind::Lance,
            PieceKind::ProKnight => PieceKind::Knight,
            PieceKind::ProSilver => PieceKind::Silver,
            PieceKind::Horse => PieceKind::Bishop,
            PieceKind::Dragon => PieceKind::Rook,
            other => other,
        }
    }

    /// 成駒か
    pub fn is_promoted(self) -> bool {
        matches!(
            self,
            PieceKind::ProPawn
                | PieceKind::ProLance
                | PieceKind::ProKnight
                | PieceKind::ProSilver
                | PieceKind::Horse
                | PieceKind::Dragon
        )
    }

    /// USI 文字（先手目線の大文字）
    pub fn usi_char(self) -> char {
        match self {
            PieceKind::Pawn => 'P',
            PieceKind::Lance => 'L',
            PieceKind::Knight => 'N',
            PieceKind::Silver => 'S',
            PieceKind::Gold => 'G',
            PieceKind::Bishop => 'B',
            PieceKind::Rook => 'R',
            PieceKind::King => 'K',
            PieceKind::ProPawn => 'P',   // +P と表記するので基本文字
            PieceKind::ProLance => 'L',
            PieceKind::ProKnight => 'N',
            PieceKind::ProSilver => 'S',
            PieceKind::Horse => 'B',
            PieceKind::Dragon => 'R',
        }
    }
}

/// 盤上の一駒
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Piece {
    pub kind: PieceKind,
    pub side: Side,
}

impl Piece {
    pub fn new(kind: PieceKind, side: Side) -> Self {
        Self { kind, side }
    }
}

/// 盤上のマス。内部は 0–80 のインデックス（筋9×段9）。
/// 筋(file): 1–9（USI: 1 が右）、段(rank): 1–9（USI: a=1, i=9）
/// index = (file-1)*9 + (rank-1)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Square(u8);

impl Square {
    /// file: 1–9, rank: 1–9
    pub fn new(file: u8, rank: u8) -> Self {
        debug_assert!((1..=9).contains(&file) && (1..=9).contains(&rank));
        Self((file - 1) * 9 + (rank - 1))
    }

    pub fn from_index(idx: u8) -> Self {
        debug_assert!(idx < 81);
        Self(idx)
    }

    pub fn index(self) -> u8 {
        self.0
    }

    /// 筋 1–9
    pub fn file(self) -> u8 {
        self.0 / 9 + 1
    }

    /// 段 1–9
    pub fn rank(self) -> u8 {
        self.0 % 9 + 1
    }

    /// USI 文字列 "7g" などへ変換
    pub fn to_usi(self) -> String {
        let rank_char = (b'a' + self.rank() - 1) as char;
        format!("{}{}", self.file(), rank_char)
    }

    /// USI 文字列 "7g" などからパース
    pub fn from_usi(s: &str) -> Option<Self> {
        let bytes = s.as_bytes();
        if bytes.len() < 2 {
            return None;
        }
        let file = bytes[0].wrapping_sub(b'0');
        let rank = bytes[1].wrapping_sub(b'a') + 1;
        if !(1..=9).contains(&file) || !(1..=9).contains(&rank) {
            return None;
        }
        Some(Self::new(file, rank))
    }
}

/// 一方のプレイヤーの一手
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    /// 盤上の駒を移動（from→to、成りフラグ付き）
    Move {
        from: Square,
        to: Square,
        promote: bool,
    },
    /// 持ち駒を打つ
    Drop { kind: PieceKind, to: Square },
}

impl Action {
    pub fn to_sq(self) -> Square {
        match self {
            Action::Move { to, .. } => to,
            Action::Drop { to, .. } => to,
        }
    }

    pub fn from_sq(self) -> Option<Square> {
        match self {
            Action::Move { from, .. } => Some(from),
            Action::Drop { .. } => None,
        }
    }

    /// USI 文字列へ変換（例: "7g7f", "7g7f+", "P*5e"）
    pub fn to_usi(self) -> String {
        match self {
            Action::Move { from, to, promote } => {
                let p = if promote { "+" } else { "" };
                format!("{}{}{}", from.to_usi(), to.to_usi(), p)
            }
            Action::Drop { kind, to } => {
                format!("{}*{}", kind.usi_char(), to.to_usi())
            }
        }
    }

    /// USI 文字列からパース（例: "7g7f", "7g7f+", "P*5e"）
    pub fn from_usi(s: &str) -> Option<Self> {
        if s.contains('*') {
            // Drop
            let (kind_str, sq_str) = s.split_once('*')?;
            let kind = parse_piece_kind_char(kind_str.chars().next()?)?;
            let to = Square::from_usi(sq_str)?;
            Some(Action::Drop { kind, to })
        } else {
            // Move
            let promote = s.ends_with('+');
            let s = if promote { &s[..s.len() - 1] } else { s };
            if s.len() < 4 {
                return None;
            }
            let from = Square::from_usi(&s[..2])?;
            let to = Square::from_usi(&s[2..4])?;
            Some(Action::Move { from, to, promote })
        }
    }
}

fn parse_piece_kind_char(c: char) -> Option<PieceKind> {
    match c.to_ascii_uppercase() {
        'P' => Some(PieceKind::Pawn),
        'L' => Some(PieceKind::Lance),
        'N' => Some(PieceKind::Knight),
        'S' => Some(PieceKind::Silver),
        'G' => Some(PieceKind::Gold),
        'B' => Some(PieceKind::Bishop),
        'R' => Some(PieceKind::Rook),
        'K' => Some(PieceKind::King),
        _ => None,
    }
}

/// 一ターン分の両陣営の着手ペア
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ply {
    pub sente: Action,
    pub gote: Action,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_roundtrip() {
        for file in 1u8..=9 {
            for rank in 1u8..=9 {
                let sq = Square::new(file, rank);
                assert_eq!(sq.file(), file);
                assert_eq!(sq.rank(), rank);
            }
        }
    }

    #[test]
    fn square_usi_roundtrip() {
        let sq = Square::new(7, 7); // "7g"
        assert_eq!(sq.to_usi(), "7g");
        assert_eq!(Square::from_usi("7g"), Some(sq));
    }

    #[test]
    fn action_usi_move() {
        let a = Action::from_usi("7g7f").unwrap();
        assert_eq!(
            a,
            Action::Move {
                from: Square::new(7, 7),
                to: Square::new(7, 6),
                promote: false
            }
        );
        assert_eq!(a.to_usi(), "7g7f");
    }

    #[test]
    fn action_usi_promote() {
        let a = Action::from_usi("2b3a+").unwrap();
        match a {
            Action::Move { promote: true, .. } => {}
            _ => panic!("expected promote"),
        }
        assert_eq!(a.to_usi(), "2b3a+");
    }

    #[test]
    fn action_usi_drop() {
        let a = Action::from_usi("P*5e").unwrap();
        assert_eq!(
            a,
            Action::Drop {
                kind: PieceKind::Pawn,
                to: Square::new(5, 5)
            }
        );
        assert_eq!(a.to_usi(), "P*5e");
    }
}
