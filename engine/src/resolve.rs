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
    // ケース1: 同一マスの相討ち（4.3）/ 戦国無双特則（4.7(ii)）
    // 片方が玉なら相討ち不適用: 玉が一方的に相手駒を取得して Z を占める
    // ----------------------------------------------------------------
    if ts == tg {
        let sente_piece = get_moving_piece(pos, sente, Side::Sente);
        let gote_piece  = get_moving_piece(pos, gote,  Side::Gote);
        return if sente_piece.kind == PieceKind::King {
            resolve_king_wins(pos, sente, gote, Side::Sente)
        } else if gote_piece.kind == PieceKind::King {
            resolve_king_wins(pos, gote, sente, Side::Gote)
        } else {
            resolve_clash(pos, sente, gote)
        };
    }

    // ----------------------------------------------------------------
    // ケース2: スワップ（4.4）/ 戦国無双特則（4.7(i)）
    // 両者が移動（打ちでない）かつ互いに相手の旧位置へ向かう
    // v0.5 §4.7 追加: 両当事者がともに玉のとき、双方の戦国無双が相殺して通常の相討ち（引き分け）
    // 片方のみ玉なら戦国無双: 玉が一方的に相手駒を取得して進む
    // ----------------------------------------------------------------
    if let (Some(fs_sq), Some(fg_sq)) = (fs, fg) {
        if ts == fg_sq && tg == fs_sq {
            let sente_piece = get_moving_piece(pos, sente, Side::Sente);
            let gote_piece  = get_moving_piece(pos, gote,  Side::Gote);
            return if sente_piece.kind == PieceKind::King && gote_piece.kind == PieceKind::King {
                // 両玉スワップ: 戦国無双が相殺 → 通常の相討ち（両玉取られて引き分け）
                resolve_clash(pos, sente, gote)
            } else if sente_piece.kind == PieceKind::King {
                resolve_king_wins(pos, sente, gote, Side::Sente)
            } else if gote_piece.kind == PieceKind::King {
                resolve_king_wins(pos, gote, sente, Side::Gote)
            } else {
                resolve_clash(pos, sente, gote)
            };
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

/// 玉が当事者の衝突で玉が勝つ（戦国無双特則 §4.7）。
///
/// スワップ（4.7(i)）と同一マス相討ちへの打ち込み（4.7(ii)）の両方を処理する。
/// 玉は取られず、相手駒を一方的に取得して移動先を占める。
/// 相手の着手は Move でも Drop でも対応する。
fn resolve_king_wins(
    pos: &Position,
    king_act: Action,
    enemy_act: Action,
    king_side: Side,
) -> Resolution {
    let king_dest = king_act.to_sq();
    let enemy_side = match king_side { Side::Sente => Side::Gote, Side::Gote => Side::Sente };

    let enemy_piece = get_moving_piece(pos, enemy_act, enemy_side);
    debug_assert!(
        enemy_piece.kind != PieceKind::King,
        "両玉スワップは resolve_clash へ分岐済み。この関数が呼ばれる時点で enemy は非玉"
    );

    let mut next = pos.clone();

    // 1. 玉の移動元を空にする（玉は必ず Move）
    let f_king = king_act.from_sq().expect("king must Move, not Drop");
    next.board.set(f_king, None);

    // 2. 敵の着手を処理（Move: 移動元を空に、Drop: 持ち駒から消費）
    match enemy_act {
        Action::Move { from, .. } => next.board.set(from, None),
        Action::Drop { kind, .. } => next.hand_mut(enemy_side).remove(kind),
        Action::Resign => unreachable!("Resign cannot be enemy_act in king wins"),
    }

    // 3. 取得した敵駒を玉側の持ち駒へ（基本種に戻す）
    next.hand_mut(king_side).add(enemy_piece.kind.unpromoted());

    // 4. 玉を移動先に置く（スワップなら敵の旧位置、同一マスなら Z を上書き）
    let king_piece = get_moving_piece(pos, king_act, king_side);
    next.board.set(king_dest, Some(king_piece));

    next.move_number += 1;

    let event = match king_side {
        Side::Sente => ResolutionEvent::Normal {
            sente_capture: Some(enemy_piece.kind),
            gote_capture: None,
        },
        Side::Gote => ResolutionEvent::Normal {
            sente_capture: None,
            gote_capture: Some(enemy_piece.kind),
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
    // sente_king_died: 後手が先手玉を取得 → gote_cap が先手玉
    // gote_king_died:  先手が後手玉を取得 → sente_cap が後手玉
    let sente_king_died = gote_cap.is_some_and(|p| p.kind == PieceKind::King);
    let gote_king_died  = sente_cap.is_some_and(|p| p.kind == PieceKind::King);

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
        Action::Resign => unreachable!("Resign has no moving piece"),
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

    /// テスト4.7-1: 戦国無双・先手玉がスワップで後手銀を一方的に取得
    #[test]
    fn sengoku_musou_sente_king_wins() {
        let king_sq   = Square::new(5, 5); // 先手玉 5五
        let silver_sq = Square::new(5, 4); // 後手銀 5四
        let pos = make_pos(&[
            (king_sq,   Piece::new(PieceKind::King,   Side::Sente)),
            (silver_sq, Piece::new(PieceKind::Silver, Side::Gote)),
            (Square::new(9, 1), Piece::new(PieceKind::King, Side::Gote)),
        ]);
        // スワップ: 先手玉 5五→5四、後手銀 5四→5五
        let sente_act = Action::Move { from: king_sq,   to: silver_sq, promote: false };
        let gote_act  = Action::Move { from: silver_sq, to: king_sq,   promote: false };
        let res = resolve(&pos, sente_act, gote_act);

        // 先手玉が 5四 にいる
        assert_eq!(res.next.board.get(silver_sq), Some(Piece::new(PieceKind::King, Side::Sente)));
        // 5五（玉の旧位置）は空（後手銀は到達しない）
        assert_eq!(res.next.board.get(king_sq), None);
        // 先手の持ち駒: 銀1枚
        assert_eq!(res.next.hand_sente.count(PieceKind::Silver), 1);
        assert_eq!(res.next.hand_gote.count(PieceKind::Silver), 0);
        // イベント: Normal（先手が銀を取得）
        assert!(matches!(
            res.event,
            ResolutionEvent::Normal { sente_capture: Some(PieceKind::Silver), gote_capture: None }
        ));
    }

    /// テスト4.7-2: 戦国無双・後手玉がスワップで先手銀を一方的に取得
    #[test]
    fn sengoku_musou_gote_king_wins() {
        let king_sq   = Square::new(5, 5); // 後手玉 5五
        let silver_sq = Square::new(5, 6); // 先手銀 5六
        let pos = make_pos(&[
            (king_sq,   Piece::new(PieceKind::King,   Side::Gote)),
            (silver_sq, Piece::new(PieceKind::Silver, Side::Sente)),
            (Square::new(9, 9), Piece::new(PieceKind::King, Side::Sente)),
        ]);
        // スワップ: 後手玉 5五→5六、先手銀 5六→5五
        let sente_act = Action::Move { from: silver_sq, to: king_sq,   promote: false };
        let gote_act  = Action::Move { from: king_sq,   to: silver_sq, promote: false };
        let res = resolve(&pos, sente_act, gote_act);

        // 後手玉が 5六 にいる
        assert_eq!(res.next.board.get(silver_sq), Some(Piece::new(PieceKind::King, Side::Gote)));
        // 5五（玉の旧位置）は空
        assert_eq!(res.next.board.get(king_sq), None);
        // 後手の持ち駒: 銀1枚
        assert_eq!(res.next.hand_gote.count(PieceKind::Silver), 1);
        assert_eq!(res.next.hand_sente.count(PieceKind::Silver), 0);
    }

    /// テスト4.7-3: 玉が留まると取られる（戦国無双はスワップ限定、主経路 5.2 は不変）
    #[test]
    fn sengoku_musou_stationary_king_dies() {
        let king_sq   = Square::new(5, 5); // 先手玉 5五（留まる）
        let silver_sq = Square::new(5, 4); // 後手銀 5四（→5五 へ取りに来る）
        let gold_sq   = Square::new(3, 9); // 先手金（先手の別の着手用）
        let pos = make_pos(&[
            (king_sq,   Piece::new(PieceKind::King,   Side::Sente)),
            (gold_sq,   Piece::new(PieceKind::Gold,   Side::Sente)),
            (silver_sq, Piece::new(PieceKind::Silver, Side::Gote)),
            (Square::new(9, 1), Piece::new(PieceKind::King, Side::Gote)),
        ]);
        // 先手: 金を動かす（玉は留まる）、後手: 銀 5四→5五（玉を取りに）
        let sente_act = Action::Move { from: gold_sq,   to: Square::new(4, 9), promote: false };
        let gote_act  = Action::Move { from: silver_sq, to: king_sq,           promote: false };
        let res = resolve(&pos, sente_act, gote_act);

        // 先手玉は取られた
        assert!(matches!(res.event, ResolutionEvent::SenteDied));
        // 後手銀が 5五 を占める
        assert_eq!(res.next.board.get(king_sq), Some(Piece::new(PieceKind::Silver, Side::Gote)));
    }

    /// テスト4.7-4: 逃げた駒は取得されない（スワップ非成立、4.2 の確認）
    #[test]
    fn sengoku_musou_escape_no_capture() {
        let king_sq   = Square::new(5, 5); // 先手玉 5五
        let silver_sq = Square::new(5, 4); // 後手銀 5四
        let escape_sq = Square::new(4, 4); // 後手銀の逃げ先（玉の旧位置でない）
        let pos = make_pos(&[
            (king_sq,   Piece::new(PieceKind::King,   Side::Sente)),
            (silver_sq, Piece::new(PieceKind::Silver, Side::Gote)),
            (Square::new(9, 1), Piece::new(PieceKind::King, Side::Gote)),
        ]);
        // 先手玉 5五→5四、後手銀 5四→4四（5五=玉の旧位置ではなく別マスへ逃げる）
        let sente_act = Action::Move { from: king_sq,   to: silver_sq, promote: false };
        let gote_act  = Action::Move { from: silver_sq, to: escape_sq, promote: false };
        let res = resolve(&pos, sente_act, gote_act);

        // 先手玉が 5四 にいる（銀は逃げた）
        assert_eq!(res.next.board.get(silver_sq), Some(Piece::new(PieceKind::King, Side::Sente)));
        // 後手銀は逃げ先 4四 にいる
        assert_eq!(res.next.board.get(escape_sq), Some(Piece::new(PieceKind::Silver, Side::Gote)));
        // 取得なし
        assert_eq!(res.next.hand_sente.count(PieceKind::Silver), 0);
    }

    /// テスト5.4: 両玉同時取得 → 引き分け（実際に起こり得る経路）
    #[test]
    fn both_kings_captured_simultaneously() {
        let sk = Square::new(5, 5); // 先手玉 5五（留まる）
        let gk = Square::new(5, 3); // 後手玉 5三（留まる）
        let sr = Square::new(5, 7); // 先手飛 5七 → 5三（後手玉を取りに）
        let gr = Square::new(5, 1); // 後手飛 5一 → 5五（先手玉を取りに）
        let pos = make_pos(&[
            (sk, Piece::new(PieceKind::King, Side::Sente)),
            (gk, Piece::new(PieceKind::King, Side::Gote)),
            (sr, Piece::new(PieceKind::Rook, Side::Sente)),
            (gr, Piece::new(PieceKind::Rook, Side::Gote)),
        ]);
        let sente_act = Action::Move { from: sr, to: gk, promote: false };
        let gote_act  = Action::Move { from: gr, to: sk, promote: false };
        let res = resolve(&pos, sente_act, gote_act);

        assert!(matches!(res.event, ResolutionEvent::BothDied));
    }

    /// テスト4.7-5: 戦国無双・先手玉の逃げ先への打ち込み（4.3 同一マス拡張）
    /// 玉が安全な空きマス X へ退避し、同時に相手が X へ持ち駒を打ち込む → 玉が勝つ
    #[test]
    fn sengoku_musou_drop_clash_sente_king() {
        let king_sq   = Square::new(5, 5); // 先手玉 5五
        let escape_sq = Square::new(5, 4); // 逃げ先 X（空きマス）
        let mut pos = make_pos(&[
            (king_sq, Piece::new(PieceKind::King, Side::Sente)),
            (Square::new(9, 1), Piece::new(PieceKind::King, Side::Gote)),
        ]);
        pos.hand_gote.add(PieceKind::Silver);

        // 先手玉 5五→5四、後手 銀を 5四 に打ち込み（同一マス衝突）
        let sente_act = Action::Move { from: king_sq, to: escape_sq, promote: false };
        let gote_act  = Action::Drop { kind: PieceKind::Silver, to: escape_sq };
        let res = resolve(&pos, sente_act, gote_act);

        // 先手玉が 5四 にいる（打ち込みに勝った）
        assert_eq!(res.next.board.get(escape_sq), Some(Piece::new(PieceKind::King, Side::Sente)));
        // 5五 は空
        assert_eq!(res.next.board.get(king_sq), None);
        // 先手の持ち駒: 銀1枚（打ち込まれた銀を取得）
        assert_eq!(res.next.hand_sente.count(PieceKind::Silver), 1);
        // 後手の持ち駒: 銀0枚（打ち込みに使い、取られた）
        assert_eq!(res.next.hand_gote.count(PieceKind::Silver), 0);
        assert!(matches!(
            res.event,
            ResolutionEvent::Normal { sente_capture: Some(PieceKind::Silver), gote_capture: None }
        ));
    }

    /// テスト4.7-6: 戦国無双・後手玉の逃げ先への打ち込み（4.3 同一マス拡張、後手玉版）
    #[test]
    fn sengoku_musou_drop_clash_gote_king() {
        let king_sq   = Square::new(5, 5); // 後手玉 5五
        let escape_sq = Square::new(5, 6); // 逃げ先 X（空きマス）
        let mut pos = make_pos(&[
            (king_sq, Piece::new(PieceKind::King, Side::Gote)),
            (Square::new(9, 9), Piece::new(PieceKind::King, Side::Sente)),
        ]);
        pos.hand_sente.add(PieceKind::Silver);

        // 後手玉 5五→5六、先手 銀を 5六 に打ち込み（同一マス衝突）
        let sente_act = Action::Drop { kind: PieceKind::Silver, to: escape_sq };
        let gote_act  = Action::Move { from: king_sq, to: escape_sq, promote: false };
        let res = resolve(&pos, sente_act, gote_act);

        // 後手玉が 5六 にいる
        assert_eq!(res.next.board.get(escape_sq), Some(Piece::new(PieceKind::King, Side::Gote)));
        assert_eq!(res.next.board.get(king_sq), None);
        // 後手の持ち駒: 銀1枚
        assert_eq!(res.next.hand_gote.count(PieceKind::Silver), 1);
        assert_eq!(res.next.hand_sente.count(PieceKind::Silver), 0);
        assert!(matches!(
            res.event,
            ResolutionEvent::Normal { sente_capture: None, gote_capture: Some(PieceKind::Silver) }
        ));
    }

    /// テスト5.2-a: 取り合いの裏目（玉は死ぬ）
    /// 先手が王手駒を別の駒で取りに行くが、その王手駒が同時に玉のマスへ移動 → 先手玉が取られる
    #[test]
    fn king_dies_counterplay_backfire() {
        let king_sq   = Square::new(5, 5); // 先手玉 5五（留まる）
        let silver_sq = Square::new(5, 4); // 後手銀 5四（王手駒）
        let gold_sq   = Square::new(6, 4); // 先手金 6四（銀を取りに行く）
        let pos = make_pos(&[
            (king_sq,   Piece::new(PieceKind::King,   Side::Sente)),
            (gold_sq,   Piece::new(PieceKind::Gold,   Side::Sente)),
            (silver_sq, Piece::new(PieceKind::Silver, Side::Gote)),
            (Square::new(9, 1), Piece::new(PieceKind::King, Side::Gote)),
        ]);
        // 先手: 金 6四→5四（銀を取りに）、後手: 銀 5四→5五（玉を取りに）
        // 後手銀が逃げながら玉を取るため resolve_independent に落ちる
        let sente_act = Action::Move { from: gold_sq,   to: silver_sq, promote: false };
        let gote_act  = Action::Move { from: silver_sq, to: king_sq,   promote: false };
        let res = resolve(&pos, sente_act, gote_act);

        // 先手玉は取られた（戦国無双は玉が動いた場合のみ適用、留まった玉は救われない）
        assert!(matches!(res.event, ResolutionEvent::SenteDied));
        // 後手銀が 5五 を占める
        assert_eq!(res.next.board.get(king_sq), Some(Piece::new(PieceKind::Silver, Side::Gote)));
        // 先手金は 5四 へ（銀の旧位置）
        assert_eq!(res.next.board.get(silver_sq), Some(Piece::new(PieceKind::Gold, Side::Sente)));
    }

    /// テスト4.7-v0.5: 両玉スワップ → 相討ち引き分け（v0.5 §4.7 追加条項）
    /// 双方の戦国無双が拮抗して相殺し、通常の相討ちに戻る → BothDied
    #[test]
    fn both_kings_swap_draws() {
        let sk = Square::new(5, 5); // 先手玉 5五
        let gk = Square::new(5, 4); // 後手玉 5四（隣接）
        let pos = make_pos(&[
            (sk, Piece::new(PieceKind::King, Side::Sente)),
            (gk, Piece::new(PieceKind::King, Side::Gote)),
        ]);
        // 両玉が互いに相手のマスへスワップ
        let sente_act = Action::Move { from: sk, to: gk, promote: false };
        let gote_act  = Action::Move { from: gk, to: sk, promote: false };
        let res = resolve(&pos, sente_act, gote_act);

        // 両玉が取られ引き分け
        assert!(matches!(res.event, ResolutionEvent::BothDied));
        // 盤上はともに空（玉は持ち駒にならない）
        assert_eq!(res.next.board.get(sk), None);
        assert_eq!(res.next.board.get(gk), None);
        assert_eq!(res.next.hand_sente.count(PieceKind::King), 0);
        assert_eq!(res.next.hand_gote.count(PieceKind::King), 0);
    }

    /// 退行防止: 片方だけ玉のスワップ（v0.4 戦国無双）が壊れていないことを確認
    /// 先手玉スワップで後手銀を取得する旧来の動作が維持される
    #[test]
    fn sengoku_musou_single_king_unaffected_after_v05() {
        let sk = Square::new(5, 5); // 先手玉 5五
        let sv = Square::new(5, 4); // 後手銀 5四（玉でない）
        let gk = Square::new(9, 1); // 後手玉（別マス）
        let pos = make_pos(&[
            (sk, Piece::new(PieceKind::King,   Side::Sente)),
            (sv, Piece::new(PieceKind::Silver, Side::Gote)),
            (gk, Piece::new(PieceKind::King,   Side::Gote)),
        ]);
        // 先手玉スワップ: 先手玉 5五→5四、後手銀 5四→5五
        let sente_act = Action::Move { from: sk, to: sv, promote: false };
        let gote_act  = Action::Move { from: sv, to: sk, promote: false };
        let res = resolve(&pos, sente_act, gote_act);

        // 戦国無双: 先手玉が 5四 にいて銀を取得
        assert_eq!(res.next.board.get(sv), Some(Piece::new(PieceKind::King, Side::Sente)));
        assert_eq!(res.next.board.get(sk), None);
        assert_eq!(res.next.hand_sente.count(PieceKind::Silver), 1);
        assert!(matches!(
            res.event,
            ResolutionEvent::Normal { sente_capture: Some(PieceKind::Silver), gote_capture: None }
        ));
    }

    /// テスト5.2-b: 合駒貫き（玉は死ぬ）
    /// 走り駒の王手に合駒するが、経路非干渉（4.6）で走り駒が合駒を素通りして玉を取る
    #[test]
    fn king_dies_rook_passthrough() {
        let king_sq  = Square::new(5, 5); // 先手玉 5五（留まる）
        let block_sq = Square::new(5, 4); // 先手が歩を合駒するマス
        let rook_sq  = Square::new(5, 2); // 後手飛 5二（5五へ走り込む）
        let mut pos = make_pos(&[
            (king_sq, Piece::new(PieceKind::King, Side::Sente)),
            (rook_sq, Piece::new(PieceKind::Rook, Side::Gote)),
            (Square::new(9, 1), Piece::new(PieceKind::King, Side::Gote)),
        ]);
        pos.hand_sente.add(PieceKind::Pawn);

        // 先手: 歩を 5四 に合駒、後手飛: 5二→5五（経路非干渉で 5四 を素通り）
        let sente_act = Action::Drop { kind: PieceKind::Pawn, to: block_sq };
        let gote_act  = Action::Move { from: rook_sq, to: king_sq, promote: false };
        let res = resolve(&pos, sente_act, gote_act);

        // 先手玉は取られた（合駒が貫かれた）
        assert!(matches!(res.event, ResolutionEvent::SenteDied));
        // 後手飛が 5五 を占める
        assert_eq!(res.next.board.get(king_sq), Some(Piece::new(PieceKind::Rook, Side::Gote)));
        // 先手歩は 5四 に着地している（玉とは別のマス）
        assert_eq!(res.next.board.get(block_sq), Some(Piece::new(PieceKind::Pawn, Side::Sente)));
        // 先手の持ち駒から歩が減った
        assert_eq!(res.next.hand_sente.count(PieceKind::Pawn), 0);
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
