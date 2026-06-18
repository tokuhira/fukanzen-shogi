/// 同時着手の三段階解決と衝突解決。
///
/// 不変条件: 同一マスの当事者となる駒は常に最大2枚（各プレイヤー1着手のみのため）。
/// 3枚以上の衝突は構造的に発生しない。
use crate::board::Position;
use crate::types::{Action, Piece, PieceKind, Side};

/// 衝突解決の結果
#[derive(Debug, Clone)]
pub struct Resolution {
    pub next: Position,
    pub event: ResolutionEvent,
}

/// ターン内で起きた主要イベント
#[derive(Debug, Clone)]
pub enum ResolutionEvent {
    /// 通常解決（取得・逃げ・空き移動など複合）
    Normal {
        sente_capture: Option<PieceKind>,
        gote_capture: Option<PieceKind>,
    },
    /// 同一マスまたはスワップによる相討ち（交換）
    Clash {
        sente_piece: PieceKind,
        gote_piece: PieceKind,
    },
    /// 先手の玉が取られた
    SenteDied,
    /// 後手の玉が取られた
    GoteDied,
    /// 両玉が同時に取られた（引き分け）
    BothDied,
}

/// resolve: 両着手が既に合法であることを前提として次局面を返す。
/// （CLI 側で合法性を確認済みのはずだが、debug_assert で再検査することを推奨）
pub fn resolve(pos: &Position, sente: Action, gote: Action) -> Resolution {
    let ts = sente.to_sq();
    let tg = gote.to_sq();
    let fs = sente.from_sq();
    let fg = gote.from_sq();

    // ----------------------------------------------------------------
    // ケース1: 同一マスの相討ち（4.3）
    // ----------------------------------------------------------------
    if ts == tg {
        return resolve_clash(pos, sente, gote);
    }

    // ----------------------------------------------------------------
    // ケース2: スワップの相討ち（4.4）
    // 両者が移動（打ちでない）かつ互いに相手の旧位置へ向かう
    // ----------------------------------------------------------------
    if let (Some(fs_sq), Some(fg_sq)) = (fs, fg) {
        if ts == fg_sq && tg == fs_sq {
            return resolve_clash(pos, sente, gote);
        }
    }

    // ----------------------------------------------------------------
    // ケース3: 独立解決
    // ----------------------------------------------------------------
    resolve_independent(pos, sente, gote)
}

/// 相討ち（交換）の解決
fn resolve_clash(pos: &Position, sente: Action, gote: Action) -> Resolution {
    let mut next = pos.clone();
    let ts = sente.to_sq();
    let tg = gote.to_sq();

    // 移動元を空にする
    if let Some(fs) = sente.from_sq() {
        next.board.set(fs, None);
    }
    if let Some(fg) = gote.from_sq() {
        next.board.set(fg, None);
    }

    // 移動先を特定（スワップの場合は ts≠tg）
    // 両駒を取得して相手の持ち駒へ
    let sente_piece = get_moving_piece(pos, sente, Side::Sente);
    let gote_piece = get_moving_piece(pos, gote, Side::Gote);

    // 玉は持ち駒にならない（終了判定で処理）
    let sente_captured = sente_piece.kind != PieceKind::King;
    let gote_captured = gote_piece.kind != PieceKind::King;

    let sente_king_died = sente_piece.kind == PieceKind::King;
    let gote_king_died = gote_piece.kind == PieceKind::King;

    if gote_captured {
        // 先手が後手の駒を得る（成りを解除して基本種へ）
        next.hand_sente.add(gote_piece.kind.unpromoted());
    }
    if sente_captured {
        // 後手が先手の駒を得る
        next.hand_gote.add(sente_piece.kind.unpromoted());
    }

    // 盤上からは除く（移動先は空のまま or スワップなら両マス）
    next.board.set(ts, None);
    if ts != tg {
        next.board.set(tg, None);
    }

    next.move_number += 1;

    let event = match (sente_king_died, gote_king_died) {
        (true, true) => ResolutionEvent::BothDied,
        (true, false) => ResolutionEvent::SenteDied,
        (false, true) => ResolutionEvent::GoteDied,
        (false, false) => ResolutionEvent::Clash {
            sente_piece: sente_piece.kind,
            gote_piece: gote_piece.kind,
        },
    };

    Resolution { next, event }
}

/// 独立解決（各着手を原子的に適用）。
///
/// 重要: 両着手は「元の盤面」を基準に同時に評価する。順次適用すると
/// 「逃げた駒」の移動先がすでに書き換わってしまうため、ここでは
/// (1) 元の盤面から取得・逃げを判定 → (2) 全変更を一括適用 の順で処理する。
fn resolve_independent(pos: &Position, sente: Action, gote: Action) -> Resolution {
    let ts = sente.to_sq();
    let tg = gote.to_sq();

    // --- 元の盤面を基準に取得・逃げを判定 ---

    // 先手の to に後手が「留まっているか」（逃げなかったか）
    let gote_vacates_ts = gote.from_sq() == Some(ts); // 後手が ts から移動して逃げた
    let sente_vacates_tg = sente.from_sq() == Some(tg); // 先手が tg から移動して逃げた

    // 先手が取る駒（元の盤面で ts に後手の駒が留まっていれば取得）
    let sente_cap: Option<Piece> = if !gote_vacates_ts {
        pos.board.get(ts).filter(|p| p.side == Side::Gote)
    } else {
        None
    };

    // 後手が取る駒
    let gote_cap: Option<Piece> = if !sente_vacates_tg {
        pos.board.get(tg).filter(|p| p.side == Side::Sente)
    } else {
        None
    };

    // --- 一括適用 ---
    let mut next = pos.clone();

    // 1. 移動元を空にする
    if let Some(fs) = sente.from_sq() {
        next.board.set(fs, None);
    }
    if let Some(fg) = gote.from_sq() {
        next.board.set(fg, None);
    }

    // 2. 取得した駒を持ち駒へ（玉は持ち駒にならない）
    let sente_king_died = sente_cap.map_or(false, |p| p.kind == PieceKind::King);
    let gote_king_died = gote_cap.map_or(false, |p| p.kind == PieceKind::King);

    if let Some(cap) = sente_cap {
        if cap.kind != PieceKind::King {
            next.hand_sente.add(cap.kind.unpromoted());
        }
    }
    if let Some(cap) = gote_cap {
        if cap.kind != PieceKind::King {
            next.hand_gote.add(cap.kind.unpromoted());
        }
    }

    // 3. 打ちは持ち駒から消費
    if let Action::Drop { kind, .. } = sente {
        next.hand_sente.remove(kind);
    }
    if let Action::Drop { kind, .. } = gote {
        next.hand_gote.remove(kind);
    }

    // 4. 駒を移動先に置く（成りを反映）
    let mut sente_piece = get_moving_piece(pos, sente, Side::Sente);
    if let Action::Move { promote: true, .. } = sente {
        sente_piece.kind = sente_piece.kind.promoted();
    }
    next.board.set(ts, Some(sente_piece));

    let mut gote_piece = get_moving_piece(pos, gote, Side::Gote);
    if let Action::Move { promote: true, .. } = gote {
        gote_piece.kind = gote_piece.kind.promoted();
    }
    next.board.set(tg, Some(gote_piece));

    next.move_number += 1;

    let event = match (sente_king_died, gote_king_died) {
        (true, true) => ResolutionEvent::BothDied,
        (true, false) => ResolutionEvent::SenteDied,
        (false, true) => ResolutionEvent::GoteDied,
        (false, false) => ResolutionEvent::Normal {
            sente_capture: sente_cap.map(|p| p.kind),
            gote_capture: gote_cap.map(|p| p.kind),
        },
    };

    Resolution { next, event }
}

/// 着手する駒（移動元の駒、または打つ駒種）を返す
fn get_moving_piece(pos: &Position, action: Action, side: Side) -> Piece {
    match action {
        Action::Move { from, .. } => {
            pos.board.get(from).expect("no piece at from")
        }
        Action::Drop { kind, .. } => Piece::new(kind, side),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{Board, Hand, Position};
    use crate::types::{Action, Piece, PieceKind, Side, Square};

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

    /// テスト9.1-1: 取得（価値非依存）先手歩が後手飛を取る
    #[test]
    fn capture_value_independent() {
        let s5e = Square::new(5, 5);
        let s5f = Square::new(5, 6);
        let pos = make_pos(&[
            (s5f, Piece::new(PieceKind::Pawn, Side::Sente)),  // 先手歩 5f
            (s5e, Piece::new(PieceKind::Rook, Side::Gote)),   // 後手飛 5e（留まる）
            (Square::new(5, 9), Piece::new(PieceKind::King, Side::Sente)),
            (Square::new(5, 1), Piece::new(PieceKind::King, Side::Gote)),
        ]);
        // 後手は玉を動かす（無関係な手）
        let gote_act = Action::Move {
            from: Square::new(5, 1),
            to: Square::new(4, 1),
            promote: false,
        };
        let sente_act = Action::Move {
            from: s5f,
            to: s5e,
            promote: false,
        };
        let res = resolve(&pos, sente_act, gote_act);
        // 後手飛は先手の持ち駒へ
        assert_eq!(res.next.hand_sente.count(PieceKind::Rook), 1);
        // 先手歩が 5e を占める
        assert_eq!(
            res.next.board.get(s5e),
            Some(Piece::new(PieceKind::Pawn, Side::Sente))
        );
    }

    /// テスト9.1-2: 逃げた駒は取られない
    #[test]
    fn escaped_piece_not_captured() {
        let x = Square::new(5, 5);
        let y = Square::new(5, 4);
        let pos = make_pos(&[
            (Square::new(5, 8), Piece::new(PieceKind::Rook, Side::Sente)), // 先手飛 → x へ
            (x, Piece::new(PieceKind::Bishop, Side::Gote)),                 // 後手角 x → y へ逃げ
            (Square::new(5, 9), Piece::new(PieceKind::King, Side::Sente)),
            (Square::new(9, 1), Piece::new(PieceKind::King, Side::Gote)),
        ]);
        let sente_act = Action::Move {
            from: Square::new(5, 8),
            to: x,
            promote: false,
        };
        let gote_act = Action::Move {
            from: x,
            to: y,
            promote: false,
        };
        let res = resolve(&pos, sente_act, gote_act);
        // 取得なし
        assert_eq!(res.next.hand_sente.count(PieceKind::Bishop), 0);
        // 先手飛が x を占める
        assert_eq!(
            res.next.board.get(x),
            Some(Piece::new(PieceKind::Rook, Side::Sente))
        );
        // 後手角が y にいる
        assert_eq!(
            res.next.board.get(y),
            Some(Piece::new(PieceKind::Bishop, Side::Gote))
        );
    }

    /// テスト9.1-3: 同一マスへの相討ち
    #[test]
    fn clash_same_square() {
        let z = Square::new(5, 5);
        let pos = make_pos(&[
            (Square::new(5, 7), Piece::new(PieceKind::Pawn, Side::Sente)),
            (Square::new(5, 3), Piece::new(PieceKind::Pawn, Side::Gote)),
            (Square::new(9, 9), Piece::new(PieceKind::King, Side::Sente)),
            (Square::new(9, 1), Piece::new(PieceKind::King, Side::Gote)),
        ]);
        let sente_act = Action::Move { from: Square::new(5, 7), to: z, promote: false };
        let gote_act = Action::Move { from: Square::new(5, 3), to: z, promote: false };
        let res = resolve(&pos, sente_act, gote_act);
        // 両駒が交換されて持ち駒へ
        assert_eq!(res.next.hand_sente.count(PieceKind::Pawn), 1);
        assert_eq!(res.next.hand_gote.count(PieceKind::Pawn), 1);
        // z は空
        assert_eq!(res.next.board.get(z), None);
    }

    /// テスト9.1-4: スワップの相討ち
    #[test]
    fn clash_swap() {
        let a = Square::new(5, 5);
        let b = Square::new(5, 6);
        let pos = make_pos(&[
            (a, Piece::new(PieceKind::Silver, Side::Sente)),
            (b, Piece::new(PieceKind::Silver, Side::Gote)),
            (Square::new(9, 9), Piece::new(PieceKind::King, Side::Sente)),
            (Square::new(9, 1), Piece::new(PieceKind::King, Side::Gote)),
        ]);
        let sente_act = Action::Move { from: a, to: b, promote: false };
        let gote_act = Action::Move { from: b, to: a, promote: false };
        let res = resolve(&pos, sente_act, gote_act);
        // 双方相討ち
        assert_eq!(res.next.hand_sente.count(PieceKind::Silver), 1);
        assert_eq!(res.next.hand_gote.count(PieceKind::Silver), 1);
        assert_eq!(res.next.board.get(a), None);
        assert_eq!(res.next.board.get(b), None);
    }

    /// テスト9.1-7: 成駒を取ったとき基本種に戻る
    #[test]
    fn capture_promoted_reverts() {
        let s5e = Square::new(5, 5);
        let s5f = Square::new(5, 6);
        let pos = make_pos(&[
            (s5f, Piece::new(PieceKind::Pawn, Side::Sente)),
            (s5e, Piece::new(PieceKind::Dragon, Side::Gote)), // 成飛（龍）
            (Square::new(9, 9), Piece::new(PieceKind::King, Side::Sente)),
            (Square::new(9, 1), Piece::new(PieceKind::King, Side::Gote)),
        ]);
        let gote_act = Action::Move {
            from: Square::new(9, 1),
            to: Square::new(8, 1),
            promote: false,
        };
        let sente_act = Action::Move { from: s5f, to: s5e, promote: false };
        let res = resolve(&pos, sente_act, gote_act);
        // 龍→飛として先手の持ち駒へ
        assert_eq!(res.next.hand_sente.count(PieceKind::Rook), 1);
        assert_eq!(res.next.hand_sente.count(PieceKind::Dragon), 0);
    }
}
