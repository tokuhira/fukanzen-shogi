use crate::commit::{make_commit, verify_commit, Commitment, Nonce};
use crate::hash::BoardHash;
use engine::types::{Action, Side};

/// reveal 時に相手へ送るデータ
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reveal {
    pub action: Action,
    pub nonce: Nonce,
    pub board_hash: BoardHash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    /// commit が揃う前に reveal を受理しようとした（順序違反）
    RevealBeforeBothCommitted,
    /// peer reveal 受信前に ack を送ろうとした
    AckBeforePeerReveal,
    /// reveal が commit と不一致（後出し）
    CommitMismatch,
    /// 相手の盤面ハッシュが自分と一致しない
    BoardHashMismatch,
    /// まだ commit していない
    NotCommittedYet,
    /// 重複 commit
    AlreadyCommitted,
    /// 重複 peer commit
    DuplicatePeerCommit,
    /// 重複 peer reveal
    DuplicatePeerReveal,
}

/// 1 ターン分のプロトコル状態機械。
///
/// 乱数（Nonce）は呼び出し側が生成して渡す（決定的テスト可能）。
/// ソケット I/O を持たない純粋ロジック。
pub struct TurnSession {
    pub local_side: Side,
    pub current_pos_hash: BoardHash,

    // 自陣営
    local_action: Option<Action>,
    local_nonce: Option<Nonce>,
    local_commit: Option<Commitment>,
    local_revealed: bool,
    local_acked: bool,

    // 相手陣営
    peer_commit: Option<Commitment>,
    peer_action: Option<Action>,
    peer_acked: bool,
}

impl TurnSession {
    pub fn new(local_side: Side, current_pos_hash: BoardHash) -> Self {
        Self {
            local_side,
            current_pos_hash,
            local_action: None,
            local_nonce: None,
            local_commit: None,
            local_revealed: false,
            local_acked: false,
            peer_commit: None,
            peer_action: None,
            peer_acked: false,
        }
    }

    /// 自分の着手を確定し commit を生成する。送信すべき Commitment を返す。
    pub fn local_commit(
        &mut self,
        action: Action,
        nonce: Nonce,
    ) -> Result<Commitment, ProtocolError> {
        if self.local_commit.is_some() {
            return Err(ProtocolError::AlreadyCommitted);
        }
        let c = make_commit(action, &nonce);
        self.local_action = Some(action);
        self.local_nonce = Some(nonce);
        self.local_commit = Some(c);
        Ok(c)
    }

    /// 相手の commit を受信して記録する。
    pub fn receive_peer_commit(&mut self, commit: Commitment) -> Result<(), ProtocolError> {
        if self.peer_commit.is_some() {
            return Err(ProtocolError::DuplicatePeerCommit);
        }
        self.peer_commit = Some(commit);
        Ok(())
    }

    /// 両者の commit が揃っているか
    pub fn both_committed(&self) -> bool {
        self.local_commit.is_some() && self.peer_commit.is_some()
    }

    /// reveal データを生成する（両者 commit 後のみ可能）。
    pub fn local_reveal(&mut self) -> Result<Reveal, ProtocolError> {
        if !self.both_committed() {
            return Err(ProtocolError::RevealBeforeBothCommitted);
        }
        let action = self.local_action.ok_or(ProtocolError::NotCommittedYet)?;
        let nonce = self.local_nonce.ok_or(ProtocolError::NotCommittedYet)?;
        self.local_revealed = true;
        Ok(Reveal {
            action,
            nonce,
            board_hash: self.current_pos_hash,
        })
    }

    /// 相手の reveal を受信して commit との照合・盤面ハッシュ検証を行う。
    pub fn receive_peer_reveal(
        &mut self,
        action: Action,
        nonce: Nonce,
        board_hash: BoardHash,
    ) -> Result<(), ProtocolError> {
        // 順序: 両者 commit が揃う前は受理しない
        if !self.both_committed() {
            return Err(ProtocolError::RevealBeforeBothCommitted);
        }
        if self.peer_action.is_some() {
            return Err(ProtocolError::DuplicatePeerReveal);
        }
        // 拘束性: (action, nonce) が peer_commit と一致するか
        if !verify_commit(&self.peer_commit.unwrap(), action, &nonce) {
            return Err(ProtocolError::CommitMismatch);
        }
        // 盤面ハッシュ相互検証
        if board_hash != self.current_pos_hash {
            return Err(ProtocolError::BoardHashMismatch);
        }
        self.peer_action = Some(action);
        Ok(())
    }

    pub fn both_revealed(&self) -> bool {
        self.local_revealed && self.peer_action.is_some()
    }

    /// peer の reveal を受け取ったことを ack する。
    pub fn local_ack(&mut self) -> Result<(), ProtocolError> {
        if self.peer_action.is_none() {
            return Err(ProtocolError::AckBeforePeerReveal);
        }
        self.local_acked = true;
        Ok(())
    }

    /// 相手の ack を記録する。
    pub fn receive_peer_ack(&mut self) {
        self.peer_acked = true;
    }

    /// 両者 ack 完了 → ターン確定
    pub fn is_complete(&self) -> bool {
        self.local_acked && self.peer_acked
    }

    /// ターン確定後に (先手着手, 後手着手) を返す。
    pub fn get_actions(&self) -> Option<(Action, Action)> {
        if !self.is_complete() {
            return None;
        }
        let local = self.local_action?;
        let peer = self.peer_action?;
        Some(match self.local_side {
            Side::Sente => (local, peer),
            Side::Gote => (peer, local),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::board_hash;
    use engine::board::Position;

    fn mv(s: &str) -> Action {
        Action::from_usi(s).unwrap()
    }
    fn session() -> TurnSession {
        let pos = Position::initial();
        TurnSession::new(Side::Sente, board_hash(&pos))
    }

    /// 順序の正しさ: 両者 commit 前に reveal は拒否
    #[test]
    fn order_peer_reveal_rejected_before_both_committed() {
        let mut s = session();
        // 自分だけ commit（相手の commit なし）
        s.local_commit(mv("7g7f"), Nonce([1u8; 32])).unwrap();

        let peer_action = mv("3c3d");
        let peer_nonce = Nonce([2u8; 32]);
        let _peer_commit = make_commit(peer_action, &peer_nonce); // commit は相手へ送らない

        // _peer_commit を receive_peer_commit せずに peer_reveal を試みる
        let result = s.receive_peer_reveal(peer_action, peer_nonce, s.current_pos_hash);
        assert_eq!(result, Err(ProtocolError::RevealBeforeBothCommitted));

        // peer_commit なしで local_reveal も拒否
        // (自分は commit 済みだが相手の commit が未着)
        let mut s2 = session();
        s2.local_commit(mv("7g7f"), Nonce([1u8; 32])).unwrap();
        // peer_commit を受信してからでないと reveal できない
        assert_eq!(
            s2.local_reveal(),
            Err(ProtocolError::RevealBeforeBothCommitted)
        );
    }

    /// 順序の正しさ: 自分だけ commit でも peer_commit 受信後なら reveal 可能
    #[test]
    fn order_reveal_allowed_after_both_committed() {
        let mut s = session();
        let action = mv("7g7f");
        let nonce = Nonce([1u8; 32]);
        s.local_commit(action, nonce).unwrap();

        let peer_action = mv("3c3d");
        let peer_nonce = Nonce([2u8; 32]);
        let peer_commit = make_commit(peer_action, &peer_nonce);
        s.receive_peer_commit(peer_commit).unwrap();

        // 両者 commit 後は reveal 可能
        let reveal = s.local_reveal().unwrap();
        assert_eq!(reveal.action, action);
    }

    /// 拘束性: peer reveal の commit 照合
    #[test]
    fn binding_commit_mismatch_detected() {
        let mut s = session();
        let action = mv("7g7f");
        let nonce = Nonce([1u8; 32]);
        s.local_commit(action, nonce).unwrap();

        let peer_action = mv("3c3d");
        let peer_nonce = Nonce([2u8; 32]);
        let peer_commit = make_commit(peer_action, &peer_nonce);
        s.receive_peer_commit(peer_commit).unwrap();

        // 別の着手で reveal しようとする（後出し）
        let wrong_action = mv("2g2f");
        let result = s.receive_peer_reveal(wrong_action, peer_nonce, s.current_pos_hash);
        assert_eq!(result, Err(ProtocolError::CommitMismatch));
    }

    /// 盤面ハッシュ相互検証: ハッシュ不一致は即エラー
    #[test]
    fn board_hash_mismatch_detected() {
        let mut s = session();
        let action = mv("7g7f");
        let nonce = Nonce([1u8; 32]);
        s.local_commit(action, nonce).unwrap();

        let peer_action = mv("3c3d");
        let peer_nonce = Nonce([2u8; 32]);
        let peer_commit = make_commit(peer_action, &peer_nonce);
        s.receive_peer_commit(peer_commit).unwrap();

        let wrong_hash = BoardHash([0u8; 32]);
        let result = s.receive_peer_reveal(peer_action, peer_nonce, wrong_hash);
        assert_eq!(result, Err(ProtocolError::BoardHashMismatch));
    }

    /// Ack 同期: 両者 ack 完了でターン確定
    #[test]
    fn ack_sync_completes_turn() {
        let pos = Position::initial();
        let hash = board_hash(&pos);
        let mut s = TurnSession::new(Side::Sente, hash);

        let local_action = mv("7g7f");
        let local_nonce = Nonce([1u8; 32]);
        s.local_commit(local_action, local_nonce).unwrap();

        let peer_action = mv("3c3d");
        let peer_nonce = Nonce([2u8; 32]);
        let peer_commit = make_commit(peer_action, &peer_nonce);
        s.receive_peer_commit(peer_commit).unwrap();

        s.local_reveal().unwrap();
        s.receive_peer_reveal(peer_action, peer_nonce, hash)
            .unwrap();

        // 自分だけ ack → まだ未完了
        s.local_ack().unwrap();
        assert!(!s.is_complete());

        // 相手 ack → 完了
        s.receive_peer_ack();
        assert!(s.is_complete());

        // 着手ペアが正しく返る（先手視点）
        let (sa, ga) = s.get_actions().unwrap();
        assert_eq!(sa, local_action);
        assert_eq!(ga, peer_action);
    }

    /// ターン完了前は get_actions が None
    #[test]
    fn get_actions_none_before_complete() {
        let mut s = session();
        s.local_commit(mv("7g7f"), Nonce([1u8; 32])).unwrap();
        assert!(s.get_actions().is_none());
    }

    /// ack 前に peer_reveal なしは ProtocolError
    #[test]
    fn ack_before_peer_reveal_fails() {
        let mut s = session();
        assert_eq!(s.local_ack(), Err(ProtocolError::AckBeforePeerReveal));
    }

    /// 後手視点では (peer, local) の順で返る
    #[test]
    fn gote_perspective_swaps_actions() {
        let pos = Position::initial();
        let hash = board_hash(&pos);
        let mut s = TurnSession::new(Side::Gote, hash);

        let local_action = mv("3c3d"); // 後手の着手
        let local_nonce = Nonce([10u8; 32]);
        s.local_commit(local_action, local_nonce).unwrap();

        let peer_action = mv("7g7f"); // 先手の着手
        let peer_nonce = Nonce([20u8; 32]);
        let peer_commit = make_commit(peer_action, &peer_nonce);
        s.receive_peer_commit(peer_commit).unwrap();
        s.local_reveal().unwrap();
        s.receive_peer_reveal(peer_action, peer_nonce, hash)
            .unwrap();
        s.local_ack().unwrap();
        s.receive_peer_ack();

        // 後手視点: (sente=peer_action, gote=local_action)
        let (sa, ga) = s.get_actions().unwrap();
        assert_eq!(sa, peer_action);
        assert_eq!(ga, local_action);
    }

    /// 先手が投了: ターン完了後に get_actions が (Resign, 通常着手) を返す
    #[test]
    fn sente_resign_completes_turn() {
        let pos = Position::initial();
        let hash = board_hash(&pos);
        let mut s = TurnSession::new(Side::Sente, hash);

        // 先手（自分）が投了を commit
        let local_nonce = Nonce([1u8; 32]);
        s.local_commit(Action::Resign, local_nonce).unwrap();

        // 後手（相手）は通常着手
        let peer_action = mv("3c3d");
        let peer_nonce = Nonce([2u8; 32]);
        let peer_commit = make_commit(peer_action, &peer_nonce);
        s.receive_peer_commit(peer_commit).unwrap();

        s.local_reveal().unwrap();
        s.receive_peer_reveal(peer_action, peer_nonce, hash)
            .unwrap();
        s.local_ack().unwrap();
        s.receive_peer_ack();

        assert!(s.is_complete());
        let (sa, ga) = s.get_actions().unwrap();
        assert_eq!(sa, Action::Resign);
        assert_eq!(ga, peer_action);
    }

    /// 両者投了: ターン完了後に get_actions が (Resign, Resign) を返す
    #[test]
    fn mutual_resign_completes_turn() {
        let pos = Position::initial();
        let hash = board_hash(&pos);
        let mut s = TurnSession::new(Side::Sente, hash);

        let local_nonce = Nonce([1u8; 32]);
        s.local_commit(Action::Resign, local_nonce).unwrap();

        let peer_nonce = Nonce([2u8; 32]);
        let peer_commit = make_commit(Action::Resign, &peer_nonce);
        s.receive_peer_commit(peer_commit).unwrap();

        s.local_reveal().unwrap();
        s.receive_peer_reveal(Action::Resign, peer_nonce, hash)
            .unwrap();
        s.local_ack().unwrap();
        s.receive_peer_ack();

        assert!(s.is_complete());
        let (sa, ga) = s.get_actions().unwrap();
        assert_eq!(sa, Action::Resign);
        assert_eq!(ga, Action::Resign);
    }

    /// 投了コミットの拘束性: resign で commit して別の着手で reveal しようとするとエラー
    #[test]
    fn resign_commit_binding() {
        let pos = Position::initial();
        let hash = board_hash(&pos);
        let mut s = TurnSession::new(Side::Sente, hash);

        let local_nonce = Nonce([1u8; 32]);
        s.local_commit(Action::Resign, local_nonce).unwrap();

        // 相手は resign で commit したが別着手で reveal しようとする
        let peer_nonce = Nonce([2u8; 32]);
        let peer_commit = make_commit(Action::Resign, &peer_nonce);
        s.receive_peer_commit(peer_commit).unwrap();

        let wrong_action = mv("3c3d");
        let result = s.receive_peer_reveal(wrong_action, peer_nonce, hash);
        assert_eq!(result, Err(ProtocolError::CommitMismatch));
    }
}
