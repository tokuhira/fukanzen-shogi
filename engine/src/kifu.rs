/// 棋譜（初期局面 + Ply 列）。
///
/// 棋譜さえあれば初期局面からエンジンを回して任意の局面を決定的に再現できる。
/// これが第二段階の中断救済と千日手判定の共通基盤となる。
use crate::board::Position;
use crate::types::Ply;

#[derive(Debug, Clone)]
pub struct Kifu {
    pub initial_position: Position,
    pub plies: Vec<Ply>,
}

impl Kifu {
    pub fn new(initial_position: Position) -> Self {
        Self {
            initial_position,
            plies: Vec::new(),
        }
    }

    pub fn push(&mut self, ply: Ply) {
        self.plies.push(ply);
    }

    /// 棋譜を初期局面から n ターン目まで再現して局面を返す。
    /// n = 0 なら初期局面、n = plies.len() なら最終局面。
    /// これが決定的な再現の唯一の源泉（§5.10）。
    pub fn replay(&self, up_to: usize) -> Position {
        let mut pos = self.initial_position.clone();
        for ply in self.plies.iter().take(up_to) {
            let res = crate::resolve::resolve(&pos, ply.sente, ply.gote);
            pos = res.next;
        }
        pos
    }

    /// 最新の局面を返す
    pub fn current(&self) -> Position {
        self.replay(self.plies.len())
    }

    /// 直前のPlyを取り消す（CLI の undo コマンド用）
    pub fn undo(&mut self) {
        self.plies.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Position;
    use crate::types::{Action, Ply, Square};

    #[test]
    fn replay_deterministic() {
        let initial = Position::initial();
        let mut kifu = Kifu::new(initial.clone());
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
        kifu.push(ply);
        let pos1 = kifu.replay(1);
        let pos2 = kifu.current();
        assert_eq!(pos1, pos2, "replay と current が一致しない");
        assert_eq!(pos1.move_number, 2);
    }
}
