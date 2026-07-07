use engine::{
    board::Position,
    types::{Action, PieceKind, Side, Square},
};

/// 着手の日本語棋譜表記を返す。
///
/// - action: 表記したい着手
/// - side:   その着手をした陣営
/// - legal_actions: 同一局面で合法な全着手（曖昧さ解消に使う）
/// - pos:    着手前の局面
///
/// 曖昧さがある場合のみ区別符（右・左・直・上・引・寄）を付加する。
/// 打ちが必要な `打` は、同種の盤上駒も同マスに到達できる場合のみ付加する。
pub fn ja_notation(
    action: &Action,
    side: Side,
    legal_actions: &[Action],
    pos: &Position,
) -> String {
    match *action {
        Action::Resign => "投了".to_string(),

        Action::Drop { kind, to } => {
            let dest = sq_ja(to);
            let name = piece_ja(kind);
            // 同種の盤上駒が（不成で）同マスへ動ける手があれば 打 が必要
            let need_uchi = legal_actions.iter().any(|a| {
                if let Action::Move {
                    from,
                    to: t,
                    promote: false,
                } = *a
                {
                    t == to && pos.board.get(from).is_some_and(|p| p.kind == kind)
                } else {
                    false
                }
            });
            if need_uchi {
                format!("{}{}打", dest, name)
            } else {
                format!("{}{}", dest, name)
            }
        }

        Action::Move { from, to, promote } => {
            let piece_kind = match pos.board.get(from) {
                Some(p) => p.kind,
                None => return action.to_usi(), // 異常系: 出発マスに駒がない
            };
            let dest = sq_ja(to);
            let name = piece_ja(piece_kind);

            // 同種駒で同一目的マスへ向かう手の出発マス一覧（自手を含む）
            let candidates: Vec<Square> = legal_actions
                .iter()
                .filter_map(|a| {
                    if let Action::Move { from: f, to: t, .. } = *a {
                        if t == to {
                            let p = pos.board.get(f)?;
                            if p.kind == piece_kind {
                                return Some(f);
                            }
                        }
                    }
                    None
                })
                .collect();

            let disambig = disambiguate(side, from, to, &candidates);

            // 成り/不成（不成は任意成りが可能だったときのみ付加）
            let promo_suffix = if promote {
                "成"
            } else if piece_kind.can_promote()
                && (in_promo_zone(side, from) || in_promo_zone(side, to))
            {
                "不成"
            } else {
                ""
            };

            format!("{}{}{}{}", dest, name, disambig, promo_suffix)
        }
    }
}

// ── 内部ヘルパー ──────────────────────────────────────────────────────────────

/// マスの日本語表記（例: Square::new(7,6) → "７六"）
fn sq_ja(sq: Square) -> String {
    // 全角数字: U+FF10 = ０、U+FF17 = ７
    let file_char = char::from_u32(0xFF10 + sq.file() as u32).unwrap_or('?');
    let rank_kanji = ["一", "二", "三", "四", "五", "六", "七", "八", "九"][sq.rank() as usize - 1];
    format!("{}{}", file_char, rank_kanji)
}

/// 駒種の日本語名
fn piece_ja(kind: PieceKind) -> &'static str {
    match kind {
        PieceKind::Pawn => "歩",
        PieceKind::Lance => "香",
        PieceKind::Knight => "桂",
        PieceKind::Silver => "銀",
        PieceKind::Gold => "金",
        PieceKind::Bishop => "角",
        PieceKind::Rook => "飛",
        PieceKind::King => "玉",
        PieceKind::ProPawn => "と",
        PieceKind::ProLance => "成香",
        PieceKind::ProKnight => "成桂",
        PieceKind::ProSilver => "成銀",
        PieceKind::Horse => "馬",
        PieceKind::Dragon => "竜",
    }
}

/// 成り駒エリア判定
fn in_promo_zone(side: Side, sq: Square) -> bool {
    match side {
        Side::Sente => sq.rank() <= 3,
        Side::Gote => sq.rank() >= 7,
    }
}

/// 曖昧さ解消の区別符を返す（不要なら空文字列）。
///
/// candidates: 同種駒が同一目的マスへ向かえる出発マス一覧（自手の from を含む）。
/// 区別符の種類: 右・左・直・上・引・寄（および右上・左引 等の組み合わせ）。
fn disambiguate(side: Side, from: Square, to: Square, candidates: &[Square]) -> String {
    if candidates.len() <= 1 {
        return String::new();
    }

    let tf = to.file();
    let tr = to.rank();

    // 右/左ラベル（同筋は None）
    let rl_of = |sq: Square| -> Option<&'static str> {
        let sf = sq.file();
        if sf == tf {
            return None;
        }
        // 先手: 筋番号が大きい方が盤の左側
        // 後手: 筋番号が小さい方が盤の左側（後手視点で反転）
        let is_left = match side {
            Side::Sente => sf > tf,
            Side::Gote => sf < tf,
        };
        Some(if is_left { "左" } else { "右" })
    };

    // 方向ラベル（上・引・寄・直）
    // 直 = 同筋からの前進移動
    let dir_of = |sq: Square| -> &'static str {
        let sf = sq.file();
        let sr = sq.rank();
        if sr == tr {
            return "寄";
        }
        let forward = match side {
            Side::Sente => sr > tr, // 先手: 段が減る方向が前
            Side::Gote => sr < tr,  // 後手: 段が増える方向が前
        };
        if forward {
            if sf == tf {
                "直"
            } else {
                "上"
            }
        } else {
            "引"
        }
    };

    let my_rl = rl_of(from);
    let my_dir = dir_of(from);

    // 右/左（または 直 の一部として dir で試みる）だけで一意か
    if let Some(rl) = my_rl {
        if candidates
            .iter()
            .filter(|&&sq| rl_of(sq) == Some(rl))
            .count()
            <= 1
        {
            return rl.to_string();
        }
    }

    // 方向（直/上/引/寄）だけで一意か
    if candidates
        .iter()
        .filter(|&&sq| dir_of(sq) == my_dir)
        .count()
        <= 1
    {
        return my_dir.to_string();
    }

    // 右左 + 方向の組み合わせ
    match my_rl {
        Some(rl) => format!("{}{}", rl, my_dir),
        None => my_dir.to_string(),
    }
}

// ── テスト ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use engine::{
        movegen::legal_actions,
        serialize::sfen_to_position,
        types::{Action, Side},
    };

    fn pos(sfen: &str) -> Position {
        sfen_to_position(sfen).expect("bad sfen")
    }

    fn legal(sfen: &str, side: Side) -> Vec<Action> {
        legal_actions(&pos(sfen), side)
    }

    // 初期局面で先手が７六歩: 区別符なし
    #[test]
    fn pawn_no_disambig() {
        let sfen = "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1";
        let p = pos(sfen);
        let la = legal(sfen, Side::Sente);
        let action = Action::from_usi("7g7f").unwrap();
        assert_eq!(ja_notation(&action, Side::Sente, &la, &p), "７六歩");
    }

    // 初期局面で先手が５八金右（４九の金）
    #[test]
    fn gold_right_disambig() {
        let sfen = "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1";
        let p = pos(sfen);
        let la = legal(sfen, Side::Sente);
        let action = Action::from_usi("4i5h").unwrap(); // ４九の金 → ５八
        assert_eq!(ja_notation(&action, Side::Sente, &la, &p), "５八金右");
    }

    // 初期局面で先手が５八金左（６九の金）
    #[test]
    fn gold_left_disambig() {
        let sfen = "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1";
        let p = pos(sfen);
        let la = legal(sfen, Side::Sente);
        let action = Action::from_usi("6i5h").unwrap(); // ６九の金 → ５八
        assert_eq!(ja_notation(&action, Side::Sente, &la, &p), "５八金左");
    }

    // 成りあり: 角が成る
    #[test]
    fn bishop_promote() {
        // 先手の角が相手陣へ成り込む局面
        let sfen = "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1";
        let p = pos(sfen);
        let la = legal(sfen, Side::Sente);
        // 角（８八）→ 2二 成り
        let action = Action::from_usi("8h2b+").unwrap();
        assert_eq!(ja_notation(&action, Side::Sente, &la, &p), "２二角成");
    }

    // 不成: 角が相手陣へ不成
    #[test]
    fn bishop_no_promote() {
        let sfen = "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1";
        let p = pos(sfen);
        let la = legal(sfen, Side::Sente);
        let action = Action::from_usi("8h2b").unwrap();
        assert_eq!(ja_notation(&action, Side::Sente, &la, &p), "２二角不成");
    }

    // 打ち（歩）: 同筋に盤上の歩がなければ 打 不要
    #[test]
    fn pawn_drop_no_uchi() {
        // 9筋だけ先手の歩がない局面（1PPPPPPPP = 9筋空き + 8〜1筋に歩）
        // 持ち駒に歩があり、9六へ打つ → 9筋に盤上の歩はないので 打 不要
        let sfen = "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/1PPPPPPPP/1B5R1/LNSGKGSNL b P 1";
        let p = pos(sfen);
        let la = legal(sfen, Side::Sente);
        let action = Action::from_usi("P*9f").unwrap();
        assert_eq!(ja_notation(&action, Side::Sente, &la, &p), "９六歩");
    }

    // 打ち（歩）: 同マスに盤上の歩も動けるなら 打 が必要
    #[test]
    fn pawn_drop_needs_uchi() {
        // 7筋に先手の歩があり、持ち駒の歩を7六へ打つ場合
        // 7七の歩が7六に動ける → 打 が必要
        let sfen = "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/1PPPPPPPP/1B5R1/LNSGKGSNL b P 1";
        let p = pos(sfen);
        let la = legal(sfen, Side::Sente);
        let action = Action::from_usi("P*8f").unwrap(); // 8筋: 盤上に8七歩がある
        assert_eq!(ja_notation(&action, Side::Sente, &la, &p), "８六歩打");
    }

    // 投了
    #[test]
    fn resign_notation() {
        let sfen = "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1";
        let p = pos(sfen);
        assert_eq!(ja_notation(&Action::Resign, Side::Sente, &[], &p), "投了");
    }

    // sq_ja のユニットテスト
    #[test]
    fn sq_ja_conversion() {
        assert_eq!(sq_ja(Square::new(7, 6)), "７六");
        assert_eq!(sq_ja(Square::new(1, 1)), "１一");
        assert_eq!(sq_ja(Square::new(9, 9)), "９九");
        assert_eq!(sq_ja(Square::new(5, 5)), "５五");
    }
}
