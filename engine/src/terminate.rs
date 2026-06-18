/// 終了判定（確定的詰み・玉の死・千日手）
use crate::board::Position;
use crate::kifu::Kifu;
use crate::movegen::legal_actions;
use crate::resolve::ResolutionEvent;
use crate::types::Side;

/// ゲームの終了状態
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameStatus {
    /// 続行中（両者とも着手可能）
    Ongoing,
    /// 先手が着手不能 → 先手の負け（確定的詰み）
    SenteLoses,
    /// 後手が着手不能 → 後手の負け（確定的詰み）
    GoteLoses,
    /// 両者同時に着手不能 → 引き分け（通常はほぼ発生しない）
    Draw,
}

/// 着手選択前に両陣営の着手可能性を確認し、ゲーム状態を返す。
///
/// 評価順序（仕様書 §6.4）:
/// (a) 着手選択前に着手不能チェック → 確定的詰み
/// (b) resolve 後に玉の死判定
pub fn check_status(pos: &Position) -> GameStatus {
    let sente_has_moves = !legal_actions(pos, Side::Sente).is_empty();
    let gote_has_moves = !legal_actions(pos, Side::Gote).is_empty();
    match (sente_has_moves, gote_has_moves) {
        (true, true) => GameStatus::Ongoing,
        (true, false) => GameStatus::GoteLoses,
        (false, true) => GameStatus::SenteLoses,
        (false, false) => GameStatus::Draw,
    }
}

/// resolve の ResolutionEvent から玉の死を判定してゲーム終了状態を返す（None = 続行）
pub fn check_king_death(event: &ResolutionEvent) -> Option<GameEnd> {
    match event {
        ResolutionEvent::SenteDied => Some(GameEnd::SenteLoses),
        ResolutionEvent::GoteDied => Some(GameEnd::GoteLoses),
        ResolutionEvent::BothDied => Some(GameEnd::Draw),
        _ => None,
    }
}

/// 千日手検出。
///
/// 同一局面（盤面＋双方の持ち駒。手数は除く）が4回出現で成立。
/// 成立時の扱いは暫定的に引き分けとする。
/// TODO: 仕様書 §7（未確定事項）— 指し直しか引き分けかは要再検討。
///       方針3（膠着打開の奨励）との整合も含め、確定は人間の判断を待つ。
pub fn check_sennichite(kifu: &Kifu) -> bool {
    use std::collections::HashMap;
    let mut counts: HashMap<(crate::board::Board, crate::board::Hand, crate::board::Hand), u32> =
        HashMap::new();

    // 初期局面から replay して各局面の内容キーを計数
    let mut pos = kifu.initial_position.clone();
    let key = pos.content_key();
    *counts.entry(key).or_insert(0) += 1;

    for ply in &kifu.plies {
        let res = crate::resolve::resolve(&pos, ply.sente, ply.gote);
        pos = res.next;
        let key = pos.content_key();
        let cnt = counts.entry(key).or_insert(0);
        *cnt += 1;
        if *cnt >= 4 {
            return true;
        }
    }
    false
}

/// ゲーム終局理由
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameEnd {
    SenteLoses,
    GoteLoses,
    Draw,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{Board, Hand, Position};
    use crate::types::{Piece, PieceKind, Side, Square};

    fn make_pos(pieces: &[(Square, Piece)]) -> Position {
        let mut board = Board::empty();
        for &(sq, p) in pieces {
            board.set(sq, Some(p));
        }
        Position {
            board,
            hand_sente: Hand::empty(),
            hand_gote: Hand::empty(),
            move_number: 1,
        }
    }

    /// テスト9.3-13: 確定的詰み局面で legal_actions が空になる
    #[test]
    fn checkmate_no_legal_actions() {
        // 先手玉が詰まされた局面（最小限）
        // 先手玉 1a、後手金 1b と 2b で詰み
        let pos = make_pos(&[
            (Square::new(1, 1), Piece::new(PieceKind::King, Side::Sente)),
            (Square::new(1, 2), Piece::new(PieceKind::Gold, Side::Gote)),
            (Square::new(2, 2), Piece::new(PieceKind::Gold, Side::Gote)),
            (Square::new(9, 9), Piece::new(PieceKind::King, Side::Gote)),
        ]);
        let actions = legal_actions(&pos, Side::Sente);
        assert!(actions.is_empty(), "詰み局面で合法手が残っている: {:?}", actions);
        assert_eq!(check_status(&pos), GameStatus::SenteLoses);
    }
}
