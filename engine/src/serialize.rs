/// USI/SFEN 入出力と正準直列化（第二段階のハッシュへの前方互換）。
///
/// SFEN の「手番」フィールドは不完全将棋に存在しないため、固定値 "b" を置いて無視する
/// （仕様書 v0.2 §3）。千日手検出および正準直列化では手番フィールドを内容に含めない。
/// ハッシュ用正準直列化: 盤面＋持ち駒＋手数（§5.7）。
/// 千日手用内容直列化: 盤面＋持ち駒のみ（手数を除く）（§6.5）。
use crate::board::{Hand, Position};
use crate::types::{Piece, PieceKind, Side, Square};

/// 平手の初期局面を表す正本 SFEN（仕様書 v0.2 §3）。
/// 手番フィールドは "b" 固定（意味を持たず無視される）。
pub const INITIAL_SFEN: &str =
    "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1";

// -------------------------------------------------------------------------
// SFEN 形式
// -------------------------------------------------------------------------

/// Position を SFEN 文字列へ変換。
/// フォーマット: "<盤面> b <持ち駒> <手数>"
/// 手番フィールドは "b" 固定（不完全将棋に手番は存在しない。仕様書 v0.2 §3）
pub fn position_to_sfen(pos: &Position) -> String {
    let board_str = board_to_sfen(pos);
    let hand_str = hand_to_sfen(pos);
    format!("{} b {} {}", board_str, hand_str, pos.move_number)
}

/// 盤面部分の SFEN 文字列（段1〜9、筋9〜1の順）
fn board_to_sfen(pos: &Position) -> String {
    let mut rows = Vec::new();
    for rank in 1u8..=9 {
        let mut row = String::new();
        let mut empty_count = 0u8;
        for file in (1u8..=9).rev() {
            let sq = Square::new(file, rank);
            match pos.board.get(sq) {
                None => empty_count += 1,
                Some(p) => {
                    if empty_count > 0 {
                        row.push_str(&empty_count.to_string());
                        empty_count = 0;
                    }
                    row.push_str(&piece_to_sfen(p));
                }
            }
        }
        if empty_count > 0 {
            row.push_str(&empty_count.to_string());
        }
        rows.push(row);
    }
    rows.join("/")
}

fn piece_to_sfen(p: Piece) -> String {
    let promoted = p.kind.is_promoted();
    let base_char = p.kind.usi_char();
    let c = match p.side {
        Side::Sente => base_char.to_ascii_uppercase(),
        Side::Gote => base_char.to_ascii_lowercase(),
    };
    if promoted {
        format!("+{}", c)
    } else {
        c.to_string()
    }
}

/// 持ち駒部分の SFEN 文字列
/// フォーマット: "S18P2p b2n" など。持ち駒なしは "-"。
/// 先手→後手の順、駒種は飛角金銀桂香歩の順（USI 準拠）
fn hand_to_sfen(pos: &Position) -> String {
    let order = [
        PieceKind::Rook,
        PieceKind::Bishop,
        PieceKind::Gold,
        PieceKind::Silver,
        PieceKind::Knight,
        PieceKind::Lance,
        PieceKind::Pawn,
    ];
    let mut s = String::new();
    for &kind in &order {
        let cnt = pos.hand_sente.count(kind);
        if cnt > 0 {
            if cnt > 1 {
                s.push_str(&cnt.to_string());
            }
            s.push(kind.usi_char().to_ascii_uppercase());
        }
    }
    for &kind in &order {
        let cnt = pos.hand_gote.count(kind);
        if cnt > 0 {
            if cnt > 1 {
                s.push_str(&cnt.to_string());
            }
            s.push(kind.usi_char().to_ascii_lowercase());
        }
    }
    if s.is_empty() {
        "-".to_string()
    } else {
        s
    }
}

// -------------------------------------------------------------------------
// 正準直列化（ハッシュ前方互換）
// -------------------------------------------------------------------------

/// ハッシュ用の正準直列化（盤面＋持ち駒＋手数）。
/// 決定的・一意。将来の SHA-256 等への入力として使う。
pub fn canonical_bytes(pos: &Position) -> Vec<u8> {
    // SFEN 文字列をバイト列として使う（ASCIIのみなので安全）
    position_to_sfen(pos).into_bytes()
}

/// 千日手判定用の内容直列化（盤面＋持ち駒のみ、手数を除く）。
/// 手番フィールドも含めない（不完全将棋に手番は存在しない。仕様書 v0.2 §3）。
pub fn content_bytes(pos: &Position) -> Vec<u8> {
    let board_str = board_to_sfen(pos);
    let hand_str = hand_to_sfen(pos);
    format!("{} {}", board_str, hand_str).into_bytes()
}

// -------------------------------------------------------------------------
// Ply の行表記（§7.3）
// -------------------------------------------------------------------------

/// Ply を "<手数>: <先手> | <後手>" 形式の文字列へ変換
pub fn ply_to_string(move_number: u32, ply: &crate::types::Ply) -> String {
    format!("{}: {} | {}", move_number, ply.sente.to_usi(), ply.gote.to_usi())
}

/// "<手数>: <先手> | <後手>" 形式から Ply をパース
pub fn ply_from_string(s: &str) -> Option<(u32, crate::types::Ply)> {
    let (num_part, rest) = s.split_once(':')?;
    let move_number: u32 = num_part.trim().parse().ok()?;
    let (sente_str, gote_str) = rest.split_once('|')?;
    let sente = crate::types::Action::from_usi(sente_str.trim())?;
    let gote = crate::types::Action::from_usi(gote_str.trim())?;
    Some((move_number, crate::types::Ply { sente, gote }))
}

// -------------------------------------------------------------------------
// 棋譜ファイルの保存・読み込み
// -------------------------------------------------------------------------

pub fn kifu_to_string(kifu: &crate::kifu::Kifu) -> String {
    let mut lines = Vec::new();
    lines.push(format!("sfen {}", position_to_sfen(&kifu.initial_position)));
    let mut move_number = kifu.initial_position.move_number;
    for ply in &kifu.plies {
        lines.push(ply_to_string(move_number, ply));
        move_number += 1;
    }
    lines.join("\n")
}

pub fn kifu_from_string(s: &str) -> Option<crate::kifu::Kifu> {
    let mut lines = s.lines();
    let first = lines.next()?;
    let sfen_str = first.strip_prefix("sfen ")?;
    let initial = sfen_to_position(sfen_str)?;
    let mut kifu = crate::kifu::Kifu::new(initial);
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (_, ply) = ply_from_string(line)?;
        kifu.push(ply);
    }
    Some(kifu)
}

/// SFEN 文字列から Position をパース。
/// 手番フィールド（parts[1]）は "b" 固定のセンチネルとして無視する（仕様書 v0.2 §3）。
pub fn sfen_to_position(s: &str) -> Option<Position> {
    let parts: Vec<&str> = s.splitn(4, ' ').collect();
    if parts.len() < 4 {
        return None;
    }
    let board_str = parts[0];
    // parts[1] は手番フィールド（"b" 固定、不完全将棋では意味を持たず無視する）
    let hand_str = parts[2];
    let move_number: u32 = parts[3].parse().ok()?;

    let board = parse_board(board_str)?;
    let (hand_sente, hand_gote) = parse_hands(hand_str)?;

    Some(Position {
        board,
        hand_sente,
        hand_gote,
        move_number,
    })
}

fn parse_board(s: &str) -> Option<crate::board::Board> {
    let mut board = crate::board::Board::empty();
    let rows: Vec<&str> = s.split('/').collect();
    if rows.len() != 9 {
        return None;
    }
    for (rank_idx, row) in rows.iter().enumerate() {
        let rank = rank_idx as u8 + 1;
        let mut file = 9i8;
        let mut chars = row.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '+' {
                let next = chars.next()?;
                let (kind, side) = parse_piece_char(next)?;
                let kind = kind.promoted();
                if file < 1 {
                    return None;
                }
                board.set(
                    Square::new(file as u8, rank),
                    Some(Piece::new(kind, side)),
                );
                file -= 1;
            } else if c.is_ascii_digit() {
                let n = c as i8 - b'0' as i8;
                file -= n;
            } else {
                let (kind, side) = parse_piece_char(c)?;
                if file < 1 {
                    return None;
                }
                board.set(
                    Square::new(file as u8, rank),
                    Some(Piece::new(kind, side)),
                );
                file -= 1;
            }
        }
    }
    Some(board)
}

fn parse_piece_char(c: char) -> Option<(PieceKind, Side)> {
    let side = if c.is_ascii_uppercase() { Side::Sente } else { Side::Gote };
    let kind = match c.to_ascii_uppercase() {
        'P' => PieceKind::Pawn,
        'L' => PieceKind::Lance,
        'N' => PieceKind::Knight,
        'S' => PieceKind::Silver,
        'G' => PieceKind::Gold,
        'B' => PieceKind::Bishop,
        'R' => PieceKind::Rook,
        'K' => PieceKind::King,
        _ => return None,
    };
    Some((kind, side))
}

fn parse_hands(s: &str) -> Option<(Hand, Hand)> {
    let mut hand_sente = Hand::empty();
    let mut hand_gote = Hand::empty();
    if s == "-" {
        return Some((hand_sente, hand_gote));
    }
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            // 複数枚: 数字の後に駒文字
            let mut num_str = c.to_string();
            while chars.peek().map_or(false, |ch| ch.is_ascii_digit()) {
                num_str.push(chars.next().unwrap());
            }
            let count: u8 = num_str.parse().ok()?;
            let piece_char = chars.next()?;
            let (kind, side) = parse_piece_char(piece_char)?;
            for _ in 0..count {
                match side {
                    Side::Sente => hand_sente.add(kind),
                    Side::Gote => hand_gote.add(kind),
                }
            }
        } else {
            let (kind, side) = parse_piece_char(c)?;
            match side {
                Side::Sente => hand_sente.add(kind),
                Side::Gote => hand_gote.add(kind),
            }
        }
    }
    Some((hand_sente, hand_gote))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Position;

    #[test]
    fn sfen_roundtrip_initial() {
        let pos = Position::initial();
        let sfen = position_to_sfen(&pos);
        let parsed = sfen_to_position(&sfen).expect("parse failed");
        assert_eq!(parsed.board, pos.board);
        assert_eq!(parsed.hand_sente, pos.hand_sente);
        assert_eq!(parsed.hand_gote, pos.hand_gote);
        assert_eq!(parsed.move_number, pos.move_number);
    }

    #[test]
    fn canonical_deterministic() {
        let pos = Position::initial();
        let b1 = canonical_bytes(&pos);
        let b2 = canonical_bytes(&pos);
        assert_eq!(b1, b2);
    }

    #[test]
    fn content_excludes_move_number() {
        let pos1 = Position::initial();
        let mut pos2 = Position::initial();
        pos2.move_number = 99;
        assert_eq!(content_bytes(&pos1), content_bytes(&pos2));
        assert_ne!(canonical_bytes(&pos1), canonical_bytes(&pos2));
    }

    #[test]
    fn initial_sfen_matches_canonical() {
        // 仕様書 v0.2 §3 の正本 SFEN と一致することを確認
        let pos = Position::initial();
        let sfen = position_to_sfen(&pos);
        assert_eq!(
            sfen, INITIAL_SFEN,
            "初期局面 SFEN が正本と一致しません\n got: {}\n want: {}",
            sfen, INITIAL_SFEN
        );
    }

    #[test]
    fn initial_sfen_parseable() {
        // 正本 SFEN 文字列を直接パースできることを確認
        let pos = sfen_to_position(INITIAL_SFEN).expect("INITIAL_SFEN のパースに失敗");
        assert_eq!(pos.move_number, 1);
        use crate::types::{Piece, PieceKind, Side, Square};
        assert_eq!(
            pos.board.get(Square::new(8, 2)),
            Some(Piece::new(PieceKind::Rook, Side::Gote)),
            "後手飛 at 8二"
        );
        assert_eq!(
            pos.board.get(Square::new(2, 2)),
            Some(Piece::new(PieceKind::Bishop, Side::Gote)),
            "後手角 at 2二"
        );
    }

    #[test]
    fn ply_roundtrip() {
        use crate::types::{Action, Ply, Square};
        let ply = Ply {
            sente: Action::Move {
                from: Square::new(7, 7),
                to: Square::new(7, 6),
                promote: false,
            },
            gote: Action::Move {
                from: Square::new(3, 3),
                to: Square::new(3, 4),
                promote: false,
            },
        };
        let s = ply_to_string(1, &ply);
        assert_eq!(s, "1: 7g7f | 3c3d");
        let (n, parsed) = ply_from_string(&s).unwrap();
        assert_eq!(n, 1);
        assert_eq!(parsed, ply);
    }
}
