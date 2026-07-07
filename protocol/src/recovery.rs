use crate::auth::{hash_secret, verify_secret, SecretHash};
use crate::hash::{board_hash, BoardHash};
use engine::board::Position;
use engine::kifu::Kifu;

/// 中断救済セッション。
///
/// 棋譜の各局面ハッシュを相手の申告ハッシュと照合し、再開点を特定する。
/// 本人認証（共有秘密ハッシュ照合）も担う。
pub struct RecoverySession {
    kifu: Kifu,
    secret_hash: SecretHash,
}

impl RecoverySession {
    pub fn new(kifu: Kifu, secret_hash: SecretHash) -> Self {
        Self { kifu, secret_hash }
    }

    pub fn new_with_secret(kifu: Kifu, secret: &[u8]) -> Self {
        Self::new(kifu, hash_secret(secret))
    }

    /// 相手の申告ハッシュが自分の棋譜のどの局面と一致するか探す。
    /// 見つかれば再開点の Position を返す。完全不一致なら None。
    pub fn find_resume_point(&self, peer_hash: BoardHash) -> Option<Position> {
        // 初期局面から順にチェック
        for n in 0..=self.kifu.plies.len() {
            let pos = self.kifu.replay(n);
            if board_hash(&pos) == peer_hash {
                return Some(pos);
            }
        }
        None
    }

    /// 再接続時: 相手が提示した秘密の本体を検証する。
    pub fn verify_identity(&self, claimed_secret: &[u8]) -> bool {
        verify_secret(claimed_secret, &self.secret_hash)
    }

    /// 自分の現局面ハッシュ（再接続申告用）
    pub fn current_hash(&self) -> BoardHash {
        board_hash(&self.kifu.current())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::board::Position;
    use engine::types::{Action, Ply, Square};

    fn make_kifu_with_plies(n: usize) -> Kifu {
        // 異なるファイルのポーン前進を順番に使う（衝突しない）
        let sente_moves = [
            (Square::new(7, 7), Square::new(7, 6)),
            (Square::new(6, 7), Square::new(6, 6)),
            (Square::new(5, 7), Square::new(5, 6)),
            (Square::new(4, 7), Square::new(4, 6)),
        ];
        let gote_moves = [
            (Square::new(3, 3), Square::new(3, 4)),
            (Square::new(4, 3), Square::new(4, 4)),
            (Square::new(5, 3), Square::new(5, 4)),
            (Square::new(6, 3), Square::new(6, 4)),
        ];
        let mut pos = Position::initial();
        let mut plies = Vec::new();
        for i in 0..n.min(sente_moves.len()) {
            let (sf, st) = sente_moves[i];
            let (gf, gt) = gote_moves[i];
            let sente_action = Action::Move {
                from: sf,
                to: st,
                promote: false,
            };
            let gote_action = Action::Move {
                from: gf,
                to: gt,
                promote: false,
            };
            // resolve して局面を進める（不完全将棋の simultaneous resolve）
            pos = engine::resolve::resolve(&pos, sente_action, gote_action).next;
            plies.push(Ply {
                sente: sente_action,
                gote: gote_action,
            });
        }
        let mut kifu = Kifu::new(Position::initial());
        for ply in plies {
            kifu.push(ply);
        }
        kifu
    }

    /// 中断救済: 初期局面のハッシュで再開点を見つける
    #[test]
    fn find_resume_at_initial() {
        let kifu = make_kifu_with_plies(3);
        let recovery = RecoverySession::new_with_secret(kifu.clone(), b"pw");
        let initial_hash = board_hash(&kifu.initial_position);
        let found = recovery.find_resume_point(initial_hash);
        assert!(found.is_some());
        assert_eq!(found.unwrap().move_number, 1);
    }

    /// 中断救済: 途中局面のハッシュで再開点を見つける
    #[test]
    fn find_resume_mid_game() {
        let kifu = make_kifu_with_plies(3);
        let recovery = RecoverySession::new_with_secret(kifu.clone(), b"pw");
        let mid_pos = kifu.replay(2);
        let mid_hash = board_hash(&mid_pos);
        let found = recovery.find_resume_point(mid_hash);
        assert!(found.is_some());
        assert_eq!(found.unwrap().move_number, mid_pos.move_number);
    }

    /// 中断救済: 棋譜にないハッシュは None
    #[test]
    fn find_resume_unknown_hash_returns_none() {
        let kifu = make_kifu_with_plies(2);
        let recovery = RecoverySession::new_with_secret(kifu, b"pw");
        let unknown_hash = BoardHash([0xABu8; 32]);
        assert!(recovery.find_resume_point(unknown_hash).is_none());
    }

    /// 本人認証: 正しい秘密は受理、誤りはリジェクト
    #[test]
    fn auth_accept_and_reject() {
        let kifu = Kifu::new(Position::initial());
        let recovery = RecoverySession::new_with_secret(kifu, b"correct_secret");
        assert!(recovery.verify_identity(b"correct_secret"));
        assert!(!recovery.verify_identity(b"wrong_secret"));
        assert!(!recovery.verify_identity(b""));
    }
}
