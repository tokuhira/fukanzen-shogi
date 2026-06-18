/// 合法手生成。
///
/// 不完全将棋の「ある側の合法着手」は、伝統的な将棋でその側が手番だと仮定したときの
/// 合法手と完全に一致する。すなわち、着手開始時点の盤面を固定し、その側の手番とみなして
/// 伝統的将棋のルールで合法手を列挙すればよい。
use crate::board::Position;
use crate::types::{Action, Piece, PieceKind, Side, Square};

// -------------------------------------------------------------------------
// 利き計算（内部ユーティリティ）
// -------------------------------------------------------------------------

/// (dfile, drank) の差分を繰り返し走る「走り駒」の利きを収集する。
/// 障害物（味方・相手）で止まる。相手駒は利き範囲に含める（取れる）。
fn slide_attacks(
    pos: &Position,
    from: Square,
    dirs: &[(i8, i8)],
    side: Side,
) -> Vec<Square> {
    let mut result = Vec::new();
    for &(df, dr) in dirs {
        let mut f = from.file() as i8 + df;
        let mut r = from.rank() as i8 + dr;
        while (1..=9).contains(&f) && (1..=9).contains(&r) {
            let sq = Square::new(f as u8, r as u8);
            match pos.board.get(sq) {
                None => {
                    result.push(sq);
                }
                Some(p) if p.side == side => {
                    break; // 味方駒でブロック（利き範囲外）
                }
                Some(_) => {
                    result.push(sq); // 相手駒：利き範囲に含めて停止
                    break;
                }
            }
            f += df;
            r += dr;
        }
    }
    result
}

/// 1マス移動の利きを収集する。
fn step_attacks(
    pos: &Position,
    from: Square,
    dirs: &[(i8, i8)],
    side: Side,
) -> Vec<Square> {
    let mut result = Vec::new();
    for &(df, dr) in dirs {
        let f = from.file() as i8 + df;
        let r = from.rank() as i8 + dr;
        if (1..=9).contains(&f) && (1..=9).contains(&r) {
            let sq = Square::new(f as u8, r as u8);
            if pos.board.get(sq).map_or(true, |p| p.side != side) {
                result.push(sq);
            }
        }
    }
    result
}

/// ある駒がある位置から利かせる全マスを返す（合法手フィルタ前）。
/// side は当該駒の陣営（障害物判定に使う）。
pub fn piece_attacks(pos: &Position, sq: Square, piece: Piece) -> Vec<Square> {
    let side = piece.side;
    // 先手視点: 前 = rank減少方向、後手は逆
    let fwd: i8 = if side == Side::Sente { -1 } else { 1 };
    match piece.kind {
        PieceKind::Pawn => step_attacks(pos, sq, &[(0, fwd)], side),
        PieceKind::Lance => slide_attacks(pos, sq, &[(0, fwd)], side),
        PieceKind::Knight => {
            let targets: Vec<(i8, i8)> = vec![(-1, 2 * fwd), (1, 2 * fwd)];
            step_attacks(pos, sq, &targets, side)
        }
        PieceKind::Silver => step_attacks(
            pos,
            sq,
            &[
                (0, fwd),
                (-1, fwd),
                (1, fwd),
                (-1, -fwd),
                (1, -fwd),
            ],
            side,
        ),
        PieceKind::Gold
        | PieceKind::ProPawn
        | PieceKind::ProLance
        | PieceKind::ProKnight
        | PieceKind::ProSilver => step_attacks(
            pos,
            sq,
            &[(0, fwd), (-1, fwd), (1, fwd), (-1, 0), (1, 0), (0, -fwd)],
            side,
        ),
        PieceKind::Bishop => slide_attacks(pos, sq, &[(-1, -1), (1, -1), (-1, 1), (1, 1)], side),
        PieceKind::Horse => {
            let mut v = slide_attacks(pos, sq, &[(-1, -1), (1, -1), (-1, 1), (1, 1)], side);
            v.extend(step_attacks(pos, sq, &[(0, 1), (0, -1), (-1, 0), (1, 0)], side));
            v
        }
        PieceKind::Rook => {
            slide_attacks(pos, sq, &[(-1, 0), (1, 0), (0, -1), (0, 1)], side)
        }
        PieceKind::Dragon => {
            let mut v =
                slide_attacks(pos, sq, &[(-1, 0), (1, 0), (0, -1), (0, 1)], side);
            v.extend(step_attacks(
                pos,
                sq,
                &[(-1, -1), (1, -1), (-1, 1), (1, 1)],
                side,
            ));
            v
        }
        PieceKind::King => step_attacks(
            pos,
            sq,
            &[
                (-1, -1),
                (0, -1),
                (1, -1),
                (-1, 0),
                (1, 0),
                (-1, 1),
                (0, 1),
                (1, 1),
            ],
            side,
        ),
    }
}

/// 相手陣営が利かせているマスの集合を返す（玉の移動合法性判定用）。
pub fn opponent_attacks(pos: &Position, side: Side) -> Vec<Square> {
    let opp = side.opposite();
    let mut attacks = Vec::new();
    for (sq, piece) in pos.board.iter() {
        if piece.side == opp {
            attacks.extend(piece_attacks(pos, sq, piece));
        }
    }
    attacks
}

// -------------------------------------------------------------------------
// 王手判定
// -------------------------------------------------------------------------

/// side の玉の位置を返す（存在しない場合は None）
pub fn king_square(pos: &Position, side: Side) -> Option<Square> {
    pos.board.iter().find_map(|(sq, p)| {
        if p.side == side && p.kind == PieceKind::King {
            Some(sq)
        } else {
            None
        }
    })
}

/// side の玉が相手の利きに晒されているか（着手開始時点の判定）
pub fn is_in_check(pos: &Position, side: Side) -> bool {
    let Some(king_sq) = king_square(pos, side) else {
        return false;
    };
    let opp = side.opposite();
    pos.board.iter().any(|(sq, piece)| {
        piece.side == opp && piece_attacks(pos, sq, piece).contains(&king_sq)
    })
}

// -------------------------------------------------------------------------
// 合法手候補生成（疑似合法手：自玉の安全を考慮しない）
// -------------------------------------------------------------------------

fn pseudo_moves(pos: &Position, side: Side) -> Vec<Action> {
    let mut actions = Vec::new();

    for (from, piece) in pos.board.iter() {
        if piece.side != side {
            continue;
        }
        let targets = piece_attacks(pos, from, piece);
        for to in targets {
            // 成り判定
            let in_promo_zone = |sq: Square| {
                if side == Side::Sente {
                    sq.rank() <= 3
                } else {
                    sq.rank() >= 7
                }
            };
            let can_promo = piece.kind.can_promote()
                && (in_promo_zone(from) || in_promo_zone(to));

            // 行き所のない駒（移動先が行き所のないマス）→ 成りを強制
            let must_promo = must_promote(piece.kind, side, to);

            if must_promo {
                if can_promo {
                    actions.push(Action::Move {
                        from,
                        to,
                        promote: true,
                    });
                }
                // 成れないなら移動自体が非合法（行き所なし）
            } else {
                actions.push(Action::Move {
                    from,
                    to,
                    promote: false,
                });
                if can_promo {
                    actions.push(Action::Move {
                        from,
                        to,
                        promote: true,
                    });
                }
            }
        }
    }

    // 打ちの候補
    let hand = pos.hand(side);
    for file in 1u8..=9 {
        for rank in 1u8..=9 {
            let to = Square::new(file, rank);
            if pos.board.get(to).is_some() {
                continue; // 駒がある場所には打てない
            }
            for &kind in crate::board::Hand::kinds() {
                if !hand.has(kind) {
                    continue;
                }
                // 行き所のない駒の禁（打ちの場合）
                if must_promote(kind, side, to) {
                    continue; // 打った後に動けない
                }
                // 二歩チェック（仮：打ち歩詰めは後で除く）
                if kind == PieceKind::Pawn {
                    let has_pawn_in_file = pos.board.iter().any(|(sq, p)| {
                        p.side == side && p.kind == PieceKind::Pawn && sq.file() == file
                    });
                    if has_pawn_in_file {
                        continue;
                    }
                }
                actions.push(Action::Drop { kind, to });
            }
        }
    }

    actions
}

/// 駒種・陣営・移動先マスの組み合わせで「行き所のない駒」になるか
fn must_promote(kind: PieceKind, side: Side, to: Square) -> bool {
    match (kind, side) {
        (PieceKind::Pawn | PieceKind::Lance, Side::Sente) => to.rank() == 1,
        (PieceKind::Pawn | PieceKind::Lance, Side::Gote) => to.rank() == 9,
        (PieceKind::Knight, Side::Sente) => to.rank() <= 2,
        (PieceKind::Knight, Side::Gote) => to.rank() >= 8,
        _ => false,
    }
}

// -------------------------------------------------------------------------
// 自玉の安全チェック（着手後に自玉が取られる位置に残らないか）
// -------------------------------------------------------------------------

/// 仮着手（打ちでない移動）を盤面に適用したコピーを返す（成り反映）
fn apply_move_temp(pos: &Position, side: Side, action: Action) -> Position {
    let mut next = pos.clone();
    match action {
        Action::Move { from, to, promote } => {
            let mut piece = next.board.get(from).expect("no piece at from");
            next.board.set(from, None);
            // 取った駒は持ち駒へ（玉は持ち駒にならない — 終了判定は別途）
            if let Some(cap) = next.board.get(to) {
                if cap.side != side && cap.kind != PieceKind::King {
                    let base = cap.kind.unpromoted();
                    next.hand_mut(side).add(base);
                }
            }
            if promote {
                piece.kind = piece.kind.promoted();
            }
            next.board.set(to, Some(piece));
        }
        Action::Drop { kind, to } => {
            next.board.set(to, Some(Piece::new(kind, side)));
            next.hand_mut(side).remove(kind);
        }
    }
    next
}

/// 着手後に自玉が取られる位置に残らないか確認（伝統的将棋の自殺手禁止）
fn is_legal_after_self_check(pos: &Position, side: Side, action: Action) -> bool {
    let after = apply_move_temp(pos, side, action);
    !is_in_check(&after, side)
}

// -------------------------------------------------------------------------
// 打ち歩詰め判定
// -------------------------------------------------------------------------

/// 歩を to に打ったとき相手を詰ます（打ち歩詰め）かどうか
fn is_uchi_fu_dzume(pos: &Position, side: Side, to: Square) -> bool {
    let mut after = pos.clone();
    after.board.set(to, Some(Piece::new(PieceKind::Pawn, side)));
    after.hand_mut(side).remove(PieceKind::Pawn);
    // 相手の合法手が0になれば打ち歩詰め
    let opp = side.opposite();
    legal_actions(&after, opp).is_empty()
}

// -------------------------------------------------------------------------
// 公開 API: 合法手生成
// -------------------------------------------------------------------------

/// side の合法着手をすべて列挙する。
///
/// 合法手 = 伝統的将棋でその側が手番とみなしたときの合法手と完全一致する。
/// 自玉が相手の利きに晒されたまま（王手放置）になる手は除かれる。
/// 打ち歩詰め・二歩・行き所のない駒も除かれる。
pub fn legal_actions(pos: &Position, side: Side) -> Vec<Action> {
    let candidates = pseudo_moves(pos, side);
    candidates
        .into_iter()
        .filter(|&action| {
            // 自玉の安全チェック
            if !is_legal_after_self_check(pos, side, action) {
                return false;
            }
            // 打ち歩詰めの禁止（歩の打ちのみ追加チェック）
            if let Action::Drop { kind: PieceKind::Pawn, to } = action {
                if is_uchi_fu_dzume(pos, side, to) {
                    return false;
                }
            }
            true
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Position;
    use crate::types::{Piece, PieceKind, Side, Square};

    fn empty_pos() -> Position {
        Position {
            board: crate::board::Board::empty(),
            hand_sente: crate::board::Hand::empty(),
            hand_gote: crate::board::Hand::empty(),
            move_number: 1,
        }
    }

    #[test]
    fn king_cannot_move_into_check() {
        // 先手玉が後手飛の利きに侵入できないことを確認
        let mut pos = empty_pos();
        pos.board.set(Square::new(5, 5), Some(Piece::new(PieceKind::King, Side::Sente)));
        // 後手飛を5段に置く → 5段全マスに利き
        pos.board.set(Square::new(1, 1), Some(Piece::new(PieceKind::Rook, Side::Gote)));
        // 後手玉（必要）
        pos.board.set(Square::new(9, 9), Some(Piece::new(PieceKind::King, Side::Gote)));

        let actions = legal_actions(&pos, Side::Sente);
        // 玉が飛の利き（1段、1筋）へ侵入する手が無いことを確認（詳細は各手の行き先で検証）
        for a in &actions {
            if let Action::Move { to, .. } = a {
                // 後手飛の利きが通るマスへは移動できないはず
                let attacked = opponent_attacks(&pos, Side::Sente);
                assert!(!attacked.contains(to), "king moved into check: {:?}", to);
            }
        }
    }

    #[test]
    fn no_nifu() {
        // 二歩のチェック: 5筋に先手の歩がいる状態で歩を5筋に打てないこと
        let mut pos = empty_pos();
        pos.board.set(Square::new(5, 9), Some(Piece::new(PieceKind::King, Side::Sente)));
        pos.board.set(Square::new(5, 1), Some(Piece::new(PieceKind::King, Side::Gote)));
        pos.board.set(Square::new(5, 7), Some(Piece::new(PieceKind::Pawn, Side::Sente)));
        pos.hand_sente.add(PieceKind::Pawn);

        let actions = legal_actions(&pos, Side::Sente);
        let nifu = actions.iter().any(|a| {
            matches!(a, Action::Drop { kind: PieceKind::Pawn, to } if to.file() == 5)
        });
        assert!(!nifu, "二歩が合法手に現れた");
    }

    #[test]
    fn initial_legal_actions_count() {
        // 初期局面での先手の合法手数（伝統的将棋と同一）
        let pos = Position::initial();
        let actions = legal_actions(&pos, Side::Sente);
        // 初期局面: 歩9手 + 角0手 + 飛0手 + 桂2手 = 30手
        assert_eq!(actions.len(), 30, "initial sente legal actions: {:?}", actions.len());
    }
}
