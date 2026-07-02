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
/// 成立時は引き分けとする（ルール仕様 v0.6 §5.6 で確定）。
/// 連続王手の千日手（反則負け）は、王手が確率的で「片方の手がすべて王手」が
/// 定義できないため v0.6 で廃止済み。反復はすべて素の引き分けへ吸収する。
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

/// 最長手数の上限（組手）。500組手を終えて決着なければ引き分け（ルール v0.6 §5.7）。
/// 手番が無いため単位は個別着手ではなく組手。基底500手ルールの読み替え。
pub const MAX_TURNS: usize = 500;

/// 500組手に達しているか（＝これ以上は最長手数の引き分け）。
pub fn check_max_turns(kifu: &Kifu) -> bool {
    kifu.plies.len() >= MAX_TURNS
}

/// 盤上で導ける終局結果（投了を除く）。archive の (ResultKind, Outcome) へ対応づく。
///
/// 投了（ルール v0.6 §5.3・§5.4）は盤面から導けないため本関数の対象外——
/// クライアントが別途注入する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Terminal {
    Ongoing,
    Loss { loser: Side, kind: LossKind },
    Draw { kind: DrawKind },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LossKind {
    Mate,
    KingDeath,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrawKind {
    MutualMate,
    BothKingsDied,
    Sennichite,
    MaxTurns,
}

/// 棋譜（履歴）から盤上の終局を評価する（ルール v0.6 §5.8 の一元評価）。
///
/// 直前の組手の解決イベントは kifu を再生して内部で取得する
/// （呼び出し側で渡さなくてよい）。
///
/// 評価順序（ルール §5・§6.4）: 玉の死 → 確定的詰み/両者不能 → 千日手 →
/// 最長手数 → 続行。決定的な結果（玉の死・確定的詰み）が引き分け
/// （千日手・最長手数）に優先する。
pub fn evaluate(kifu: &Kifu) -> Terminal {
    // 1. 直前の組手による玉の死（5.2）。plies が空なら初期局面なのでスキップ。
    if let Some(last) = kifu.plies.last() {
        let pre = kifu.replay(kifu.plies.len() - 1);
        let res = crate::resolve::resolve(&pre, last.sente, last.gote);
        match check_king_death(&res.event) {
            Some(GameEnd::SenteLoses) => {
                return Terminal::Loss { loser: Side::Sente, kind: LossKind::KingDeath }
            }
            Some(GameEnd::GoteLoses) => {
                return Terminal::Loss { loser: Side::Gote, kind: LossKind::KingDeath }
            }
            Some(GameEnd::Draw) => return Terminal::Draw { kind: DrawKind::BothKingsDied },
            None => {}
        }
    }
    // 2. 現局面での着手不能（5.1 / 両者不能 5.4）。
    match check_status(&kifu.current()) {
        GameStatus::SenteLoses => return Terminal::Loss { loser: Side::Sente, kind: LossKind::Mate },
        GameStatus::GoteLoses => return Terminal::Loss { loser: Side::Gote, kind: LossKind::Mate },
        GameStatus::Draw => return Terminal::Draw { kind: DrawKind::MutualMate },
        GameStatus::Ongoing => {}
    }
    // 3. 千日手（5.6）。
    if check_sennichite(kifu) {
        return Terminal::Draw { kind: DrawKind::Sennichite };
    }
    // 4. 最長手数（5.7）。
    if check_max_turns(kifu) {
        return Terminal::Draw { kind: DrawKind::MaxTurns };
    }
    Terminal::Ongoing
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

    // ── ルール v0.6: 最長手数・一元評価（evaluate） ──────────────────────────

    use crate::kifu::Kifu;
    use crate::types::{Action, Ply};

    /// board を持たない衝突なしの kifu を作る。sente 王は ranks 1-4、gote 王は
    /// ranks 6-9 の別々の半面をそれぞれ独立した周期でシャッフルし、両者の
    /// 組み合わせ（盤面内容）が周期 lcm(36,35)=1260 で繰り返す。500組手までは
    /// 一度も重複しないため、千日手を誤検出せず最長手数だけを試験できる。
    fn half_board_squares(rank_lo: u8, rank_hi: u8) -> Vec<Square> {
        let mut v = Vec::new();
        for rank in rank_lo..=rank_hi {
            for file in 1..=9u8 {
                v.push(Square::new(file, rank));
            }
        }
        v
    }

    fn no_repeat_kifu(n_plies: usize) -> Kifu {
        let sente_squares = half_board_squares(1, 4); // 36マス
        let gote_squares: Vec<Square> = half_board_squares(6, 9).into_iter().take(35).collect(); // 35マス

        let initial = make_pos(&[
            (sente_squares[0], Piece::new(PieceKind::King, Side::Sente)),
            (gote_squares[0], Piece::new(PieceKind::King, Side::Gote)),
        ]);
        let mut kifu = Kifu::new(initial);

        let mut sente_at = 0usize;
        let mut gote_at = 0usize;
        for i in 0..n_plies {
            let next_sente = (i + 1) % sente_squares.len();
            let next_gote = (i + 1) % gote_squares.len();
            kifu.push(Ply {
                sente: Action::Move {
                    from: sente_squares[sente_at],
                    to: sente_squares[next_sente],
                    promote: false,
                },
                gote: Action::Move {
                    from: gote_squares[gote_at],
                    to: gote_squares[next_gote],
                    promote: false,
                },
            });
            sente_at = next_sente;
            gote_at = next_gote;
        }
        kifu
    }

    #[test]
    fn max_turns_boundary() {
        assert!(!check_max_turns(&no_repeat_kifu(MAX_TURNS - 1)));
        assert!(check_max_turns(&no_repeat_kifu(MAX_TURNS)));
    }

    #[test]
    fn evaluate_ongoing_at_start() {
        assert_eq!(evaluate(&no_repeat_kifu(0)), Terminal::Ongoing);
    }

    #[test]
    fn evaluate_king_death_takes_priority() {
        // テスト4.7-3（resolve.rs）と同型: 玉は留まり、別の駒を動かす。
        // 相手の駒がその玉のマスへ来て直接取得する ＝ 玉の死。
        let king_sq = Square::new(5, 5);
        let gold_sq = Square::new(3, 9);
        let rook_sq = Square::new(5, 1);
        let initial = make_pos(&[
            (king_sq, Piece::new(PieceKind::King, Side::Sente)),
            (gold_sq, Piece::new(PieceKind::Gold, Side::Sente)),
            (rook_sq, Piece::new(PieceKind::Rook, Side::Gote)),
            (Square::new(9, 9), Piece::new(PieceKind::King, Side::Gote)),
        ]);
        let mut kifu = Kifu::new(initial);
        kifu.push(Ply {
            sente: Action::Move { from: gold_sq, to: Square::new(4, 9), promote: false },
            gote: Action::Move { from: rook_sq, to: king_sq, promote: false },
        });
        assert_eq!(
            evaluate(&kifu),
            Terminal::Loss { loser: Side::Sente, kind: LossKind::KingDeath }
        );
    }

    #[test]
    fn evaluate_mate() {
        let pos = make_pos(&[
            (Square::new(1, 1), Piece::new(PieceKind::King, Side::Sente)),
            (Square::new(1, 2), Piece::new(PieceKind::Gold, Side::Gote)),
            (Square::new(2, 2), Piece::new(PieceKind::Gold, Side::Gote)),
            (Square::new(9, 9), Piece::new(PieceKind::King, Side::Gote)),
        ]);
        let kifu = Kifu::new(pos);
        assert_eq!(
            evaluate(&kifu),
            Terminal::Loss { loser: Side::Sente, kind: LossKind::Mate }
        );
    }

    #[test]
    fn evaluate_mutual_mate() {
        // 点対称の写像で checkmate_no_legal_actions を後手側にも複製し、
        // 両者が同時に着手不能な局面を作る。
        let pos = make_pos(&[
            (Square::new(1, 1), Piece::new(PieceKind::King, Side::Sente)),
            (Square::new(1, 2), Piece::new(PieceKind::Gold, Side::Gote)),
            (Square::new(2, 2), Piece::new(PieceKind::Gold, Side::Gote)),
            (Square::new(9, 9), Piece::new(PieceKind::King, Side::Gote)),
            (Square::new(9, 8), Piece::new(PieceKind::Gold, Side::Sente)),
            (Square::new(8, 8), Piece::new(PieceKind::Gold, Side::Sente)),
        ]);
        let kifu = Kifu::new(pos);
        assert_eq!(evaluate(&kifu), Terminal::Draw { kind: DrawKind::MutualMate });
    }

    #[test]
    fn evaluate_sennichite() {
        // 先後の玉を隣接2マス間で往復させ、初期局面と同一の内容が4回出現させる。
        let a_sente = Square::new(1, 1);
        let b_sente = Square::new(2, 1);
        let a_gote = Square::new(9, 9);
        let b_gote = Square::new(8, 9);
        let initial = make_pos(&[
            (a_sente, Piece::new(PieceKind::King, Side::Sente)),
            (a_gote, Piece::new(PieceKind::King, Side::Gote)),
        ]);
        let mut kifu = Kifu::new(initial);
        for i in 0..6 {
            let ply = if i % 2 == 0 {
                Ply {
                    sente: Action::Move { from: a_sente, to: b_sente, promote: false },
                    gote: Action::Move { from: a_gote, to: b_gote, promote: false },
                }
            } else {
                Ply {
                    sente: Action::Move { from: b_sente, to: a_sente, promote: false },
                    gote: Action::Move { from: b_gote, to: a_gote, promote: false },
                }
            };
            kifu.push(ply);
        }
        assert_eq!(evaluate(&kifu), Terminal::Draw { kind: DrawKind::Sennichite });
    }

    #[test]
    fn evaluate_max_turns() {
        assert_eq!(
            evaluate(&no_repeat_kifu(MAX_TURNS)),
            Terminal::Draw { kind: DrawKind::MaxTurns }
        );
    }
}
