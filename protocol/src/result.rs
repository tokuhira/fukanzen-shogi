//! 終局判定の単一窓口。投了（プロトコルの範疇）と盤面終局（engine）を合流させる。
//! 投了は本将棋/USI でも着手でなく宣言なので、ここ（protocol 層）で先に捌き、
//! 盤面終局は engine の evaluate + terminal_to_result へ委譲する（アーク概観 §1-2）。

use engine::archive::{Outcome, ResultKind};
use engine::kifu::Kifu;
use engine::terminate::{evaluate, terminal_to_result};

/// 対局の終局結果。対局中（未了）は `None`。
///
/// 1. 最後の組手が投了なら、投了として勝敗を返す（盤面に依らず・投了優先）。
///    投了組手を先に捌くことで、後段の `evaluate` に投了組手を渡さない
///    （`evaluate` は投了組手で panic する＝Step A の前提）。
/// 2. 投了でなければ engine の盤面終局へ委譲（`Ongoing` は `None`）。
pub fn game_result(kifu: &Kifu) -> Option<(ResultKind, Outcome)> {
    // 1. 投了（protocol の領分)。投了は必ず最後の組手（そこで対局が終わる）。
    if let Some(last) = kifu.plies.last() {
        match (last.sente.is_resign(), last.gote.is_resign()) {
            (true, true) => return Some((ResultKind::Resign, Outcome::Draw)), // 両者投了（v0.6 §5.4）
            (true, false) => return Some((ResultKind::Resign, Outcome::GoteWins)), // 先手投了 → 後手勝ち
            (false, true) => return Some((ResultKind::Resign, Outcome::SenteWins)), // 後手投了 → 先手勝ち
            (false, false) => {}
        }
    }
    // 2. 盤面終局は engine へ委譲（Ongoing なら None）。
    terminal_to_result(&evaluate(kifu))
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::serialize::{sfen_to_position, INITIAL_SFEN};
    use engine::types::{Action, Ply, Square};

    fn initial_kifu() -> Kifu {
        Kifu::new(sfen_to_position(INITIAL_SFEN).unwrap())
    }

    fn legal_ply() -> Ply {
        Ply {
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
        }
    }

    #[test]
    fn sente_resign_gote_wins() {
        let mut kifu = initial_kifu();
        kifu.push(Ply {
            sente: Action::Resign,
            gote: legal_ply().gote,
        });
        assert_eq!(
            game_result(&kifu),
            Some((ResultKind::Resign, Outcome::GoteWins))
        );
    }

    #[test]
    fn gote_resign_sente_wins() {
        let mut kifu = initial_kifu();
        kifu.push(Ply {
            sente: legal_ply().sente,
            gote: Action::Resign,
        });
        assert_eq!(
            game_result(&kifu),
            Some((ResultKind::Resign, Outcome::SenteWins))
        );
    }

    #[test]
    fn both_resign_draw() {
        let mut kifu = initial_kifu();
        kifu.push(Ply {
            sente: Action::Resign,
            gote: Action::Resign,
        });
        assert_eq!(
            game_result(&kifu),
            Some((ResultKind::Resign, Outcome::Draw))
        );
    }

    #[test]
    fn ongoing_kifu_returns_none() {
        let kifu = initial_kifu();
        assert_eq!(game_result(&kifu), None);
    }

    #[test]
    fn delegates_to_board_evaluation_when_not_resigned() {
        let kifu = initial_kifu();
        assert_eq!(game_result(&kifu), terminal_to_result(&evaluate(&kifu)));

        let mut kifu_with_move = initial_kifu();
        kifu_with_move.push(legal_ply());
        assert_eq!(
            game_result(&kifu_with_move),
            terminal_to_result(&evaluate(&kifu_with_move))
        );
    }
}
