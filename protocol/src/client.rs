//! セッション orchestration（純粋・transport 非依存）。
//!
//! 出典: protocol-wasm の `ProtocolSession`。同じ状態遷移を typed で再現する
//! （handshake の版交渉＋peer_auth_hash 記録・commit の先着バッファ・
//! ack 完了時の turn 解放）。再接続だけは本人照合を核へ引き上げて再定義する
//! （生 secret を晒さず auth_hash 一致で照合、再開点確認は殻に委ねる）。

use crate::auth::{hash_secret, SecretHash};
use crate::commit::{Commitment, Nonce};
use crate::hash::BoardHash;
use crate::negotiate::{
    negotiate_versions, NegotiationOutcome, PeerVersionResponse, VersionTuple, MY_VERSION,
};
use crate::session::{ProtocolError, TurnSession};
use crate::wire::WireMessage;
use engine::types::{Action, Side};

pub struct ClientSession {
    side: Side,
    my_auth_hash: SecretHash,
    peer_auth_hash: Option<SecretHash>, // 初回 hello で記録（再接続照合に使う）
    handshake_done: bool,
    turn: Option<TurnSession>,
    pending_peer_commit: Option<Commitment>, // 自分の commit より先に届いた peer commit
}

/// feed() が返す状態変化。殻はこれを見て UI 更新・次アクションを決める。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    HandshakeDone { peer_side: Side },
    PeerCommitted { both_committed: bool },
    PeerCommitBuffered,
    PeerRevealed { both_revealed: bool },
    PeerAcked,
    TurnComplete { sente: Action, gote: Action },
    /// 相手の再接続要求（本人照合は通過済み）。殻が board_hash で再開点を確認し、
    /// 良ければ reconnect_ack_msg(resume) を送る。
    PeerReconnectRequest { board_hash: BoardHash },
    /// 自分の再接続が承認された。resume_hash が再開点。
    ReconnectAck { resume_hash: BoardHash },
    PeerAborted { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    VersionMismatch(NegotiationOutcome),
    DuplicateHello,
    HandshakeNotDone,
    NoActiveTurn,
    Protocol(ProtocolError),
    IdentityMismatch, // 再接続 auth_hash が初回 peer_auth_hash と不一致
    BadHex,            // hex 欄が不正
    InvalidUsi,         // action 欄が USI として不正
}

impl ClientSession {
    /// side: 殻が決めた自陣営（LAN=ポータル選択, cloud=DO の room_ready/peer_joined）。
    pub fn new(side: Side, secret: &[u8]) -> Self {
        Self {
            side,
            my_auth_hash: hash_secret(secret),
            peer_auth_hash: None,
            handshake_done: false,
            turn: None,
            pending_peer_commit: None,
        }
    }

    pub fn handshake_done(&self) -> bool {
        self.handshake_done
    }

    pub fn peer_auth_hash(&self) -> Option<SecretHash> {
        self.peer_auth_hash
    }

    /// 接続直後に相手へ送る hello（版＋auth_hash＋side）。
    pub fn hello_msg(&self) -> WireMessage {
        WireMessage::Hello {
            rule_major: MY_VERSION.rule.0,
            rule_minor: MY_VERSION.rule.1,
            proto_ver: MY_VERSION.protocol,
            auth_hash: crate::wire::to_hex(&self.my_auth_hash.0),
            side: side_str(self.side).to_string(),
        }
    }

    /// 再接続時に送る（現局面 board_hash・生 secret は晒さない）。
    pub fn reconnect_msg(&self, board_hash: BoardHash) -> WireMessage {
        WireMessage::Reconnect {
            auth_hash: crate::wire::to_hex(&self.my_auth_hash.0),
            board_hash: crate::wire::to_hex(&board_hash.0),
        }
    }

    /// 残留側が再接続を承認するときに送る（合意した再開点）。
    pub fn reconnect_ack_msg(&self, resume_hash: BoardHash) -> WireMessage {
        WireMessage::ReconnectAck {
            board_hash: crate::wire::to_hex(&resume_hash.0),
        }
    }

    /// 自分の着手を確定し commit を生成。新 TurnSession を張り、
    /// 先着していた peer commit があれば適用する（出典: commit_move）。
    /// board_hash・action・nonce は呼び出し側が用意（核は sfen/usi 解釈・乱数を持たない）。
    pub fn commit(
        &mut self,
        board_hash: BoardHash,
        action: Action,
        nonce: Nonce,
    ) -> Result<WireMessage, SessionError> {
        if !self.handshake_done {
            return Err(SessionError::HandshakeNotDone);
        }
        let mut t = TurnSession::new(self.side, board_hash);
        let commitment = t.local_commit(action, nonce).map_err(SessionError::Protocol)?;
        if let Some(pc) = self.pending_peer_commit.take() {
            // 先着 peer commit。出典（commit_move）は `let _ =` で無視している。挙動保存のため無視する。
            let _ = t.receive_peer_commit(pc);
        }
        self.turn = Some(t);
        Ok(WireMessage::Commit {
            commitment: crate::wire::to_hex(&commitment.0),
        })
    }

    pub fn both_committed(&self) -> bool {
        self.turn.as_ref().map(|t| t.both_committed()).unwrap_or(false)
    }

    /// 両者 commit 後に reveal を生成（出典: reveal_msg）。
    pub fn reveal_msg(&mut self) -> Result<WireMessage, SessionError> {
        let t = self.turn.as_mut().ok_or(SessionError::NoActiveTurn)?;
        let r = t.local_reveal().map_err(SessionError::Protocol)?;
        Ok(WireMessage::Reveal {
            action: r.action.to_usi(),
            nonce: crate::wire::to_hex(&r.nonce.0),
            board_hash: crate::wire::to_hex(&r.board_hash.0),
        })
    }

    /// peer reveal 検証後に ack を生成（出典: ack_msg）。
    pub fn ack_msg(&mut self) -> Result<WireMessage, SessionError> {
        let t = self.turn.as_mut().ok_or(SessionError::NoActiveTurn)?;
        t.local_ack().map_err(SessionError::Protocol)?;
        Ok(WireMessage::Ack)
    }

    pub fn feed(&mut self, msg: WireMessage) -> Result<SessionEvent, SessionError> {
        match msg {
            WireMessage::Hello {
                rule_major,
                rule_minor,
                proto_ver,
                auth_hash,
                side,
            } => {
                if self.handshake_done {
                    return Err(SessionError::DuplicateHello);
                }
                let peer = VersionTuple {
                    rule: (rule_major, rule_minor),
                    protocol: proto_ver,
                };
                negotiate_versions(&MY_VERSION, PeerVersionResponse::Version(peer))
                    .map_err(SessionError::VersionMismatch)?;
                let ah = parse_hash(&auth_hash)?; // 不正 hex は BadHex
                self.peer_auth_hash = Some(ah);
                self.handshake_done = true;
                Ok(SessionEvent::HandshakeDone {
                    peer_side: parse_side(&side),
                })
            }
            WireMessage::Commit { commitment } => {
                let c = Commitment(parse_bytes32(&commitment)?);
                if let Some(t) = self.turn.as_mut() {
                    t.receive_peer_commit(c).map_err(SessionError::Protocol)?;
                    Ok(SessionEvent::PeerCommitted {
                        both_committed: t.both_committed(),
                    })
                } else {
                    self.pending_peer_commit = Some(c); // 先着バッファ
                    Ok(SessionEvent::PeerCommitBuffered)
                }
            }
            WireMessage::Reveal {
                action,
                nonce,
                board_hash,
            } => {
                let t = self.turn.as_mut().ok_or(SessionError::NoActiveTurn)?;
                let a = Action::from_usi(&action).ok_or(SessionError::InvalidUsi)?;
                let n = Nonce(parse_bytes32(&nonce)?);
                let bh = BoardHash(parse_bytes32(&board_hash)?);
                t.receive_peer_reveal(a, n, bh).map_err(SessionError::Protocol)?;
                Ok(SessionEvent::PeerRevealed {
                    both_revealed: t.both_revealed(),
                })
            }
            WireMessage::Ack => {
                let t = self.turn.as_mut().ok_or(SessionError::NoActiveTurn)?;
                t.receive_peer_ack();
                if t.is_complete() {
                    if let Some((sente, gote)) = t.get_actions() {
                        self.turn = None; // 次ターンの先着 commit を feed が正しくバッファできるよう解放（出典と一致）
                        return Ok(SessionEvent::TurnComplete { sente, gote });
                    }
                }
                Ok(SessionEvent::PeerAcked)
            }
            WireMessage::Reconnect {
                auth_hash,
                board_hash,
            } => {
                // 本人照合: 相手が名乗る auth_hash が初回 hello の peer_auth_hash と一致するか。
                let claimed = parse_hash(&auth_hash)?;
                match self.peer_auth_hash {
                    Some(stored) if stored == claimed => {
                        let bh = BoardHash(parse_bytes32(&board_hash)?);
                        Ok(SessionEvent::PeerReconnectRequest { board_hash: bh })
                    }
                    _ => Err(SessionError::IdentityMismatch),
                }
            }
            WireMessage::ReconnectAck { board_hash } => {
                let bh = BoardHash(parse_bytes32(&board_hash)?);
                Ok(SessionEvent::ReconnectAck { resume_hash: bh })
            }
            WireMessage::Abort { reason } => Ok(SessionEvent::PeerAborted { reason }),
        }
    }
}

fn side_str(s: Side) -> &'static str {
    match s {
        Side::Sente => "sente",
        Side::Gote => "gote",
    }
}
fn parse_side(s: &str) -> Side {
    if s == "sente" {
        Side::Sente
    } else {
        Side::Gote
    }
}

fn parse_bytes32(hex: &str) -> Result<[u8; 32], SessionError> {
    crate::wire::from_hex32(hex).ok_or(SessionError::BadHex)
}
fn parse_hash(hex: &str) -> Result<SecretHash, SessionError> {
    Ok(SecretHash(parse_bytes32(hex)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::board_hash;
    use engine::board::Position;

    fn mv(s: &str) -> Action {
        Action::from_usi(s).unwrap()
    }

    fn initial_hash() -> BoardHash {
        board_hash(&Position::initial())
    }

    fn pair() -> (ClientSession, ClientSession) {
        let sente = ClientSession::new(Side::Sente, b"shared_secret");
        let gote = ClientSession::new(Side::Gote, b"shared_secret");
        (sente, gote)
    }

    fn handshake(sente: &mut ClientSession, gote: &mut ClientSession) {
        let hello_s = sente.hello_msg();
        let hello_g = gote.hello_msg();
        let ev_g = gote.feed(hello_s).unwrap();
        let ev_s = sente.feed(hello_g).unwrap();
        assert_eq!(ev_g, SessionEvent::HandshakeDone { peer_side: Side::Sente });
        assert_eq!(ev_s, SessionEvent::HandshakeDone { peer_side: Side::Gote });
    }

    /// 完全な一局: hello 交換 → commit → reveal → ack → 双方 TurnComplete。
    #[test]
    fn full_turn_completes_for_both_sides() {
        let (mut sente, mut gote) = pair();
        handshake(&mut sente, &mut gote);

        let bh = initial_hash();
        let commit_s = sente.commit(bh, mv("7g7f"), Nonce([1u8; 32])).unwrap();
        let commit_g = gote.commit(bh, mv("3c3d"), Nonce([2u8; 32])).unwrap();

        let ev_g = gote.feed(commit_s).unwrap();
        let ev_s = sente.feed(commit_g).unwrap();
        assert_eq!(ev_g, SessionEvent::PeerCommitted { both_committed: true });
        assert_eq!(ev_s, SessionEvent::PeerCommitted { both_committed: true });
        assert!(sente.both_committed());
        assert!(gote.both_committed());

        let reveal_s = sente.reveal_msg().unwrap();
        let reveal_g = gote.reveal_msg().unwrap();
        let ev_g = gote.feed(reveal_s).unwrap();
        let ev_s = sente.feed(reveal_g).unwrap();
        assert_eq!(ev_g, SessionEvent::PeerRevealed { both_revealed: true });
        assert_eq!(ev_s, SessionEvent::PeerRevealed { both_revealed: true });

        let ack_s = sente.ack_msg().unwrap();
        let ack_g = gote.ack_msg().unwrap();
        let ev_g = gote.feed(ack_s).unwrap();
        let ev_s = sente.feed(ack_g).unwrap();
        assert_eq!(
            ev_g,
            SessionEvent::TurnComplete { sente: mv("7g7f"), gote: mv("3c3d") }
        );
        assert_eq!(
            ev_s,
            SessionEvent::TurnComplete { sente: mv("7g7f"), gote: mv("3c3d") }
        );
    }

    /// 先着バッファ: 自分が commit する前に相手の Commit が届いたらバッファされ、
    /// 自分が commit した時点で適用される。
    #[test]
    fn peer_commit_arriving_early_is_buffered_then_applied() {
        let (mut sente, mut gote) = pair();
        handshake(&mut sente, &mut gote);

        let bh = initial_hash();
        let commit_s = sente.commit(bh, mv("7g7f"), Nonce([1u8; 32])).unwrap();

        // gote はまだ commit していない状態で sente の commit を受け取る。
        let ev = gote.feed(commit_s).unwrap();
        assert_eq!(ev, SessionEvent::PeerCommitBuffered);
        assert!(!gote.both_committed());

        let _commit_g = gote.commit(bh, mv("3c3d"), Nonce([2u8; 32])).unwrap();
        assert!(gote.both_committed());
    }

    /// 版不一致: proto_ver が食い違う hello は VersionMismatch。
    #[test]
    fn version_mismatch_rejected() {
        let mut gote = ClientSession::new(Side::Gote, b"shared_secret");
        let bad_hello = WireMessage::Hello {
            rule_major: MY_VERSION.rule.0,
            rule_minor: MY_VERSION.rule.1,
            proto_ver: MY_VERSION.protocol + 1,
            auth_hash: crate::wire::to_hex(&[0u8; 32]),
            side: "sente".to_string(),
        };
        let result = gote.feed(bad_hello);
        assert!(matches!(result, Err(SessionError::VersionMismatch(_))));
    }

    /// 投了: commit(Resign) が TurnComplete に Resign を含んで通る。
    #[test]
    fn resign_completes_turn() {
        let (mut sente, mut gote) = pair();
        handshake(&mut sente, &mut gote);

        let bh = initial_hash();
        let commit_s = sente.commit(bh, Action::Resign, Nonce([1u8; 32])).unwrap();
        let commit_g = gote.commit(bh, mv("3c3d"), Nonce([2u8; 32])).unwrap();
        gote.feed(commit_s).unwrap();
        sente.feed(commit_g).unwrap();

        let reveal_s = sente.reveal_msg().unwrap();
        let reveal_g = gote.reveal_msg().unwrap();
        gote.feed(reveal_s).unwrap();
        sente.feed(reveal_g).unwrap();

        let ack_s = sente.ack_msg().unwrap();
        let ack_g = gote.ack_msg().unwrap();
        gote.feed(ack_s).unwrap();
        let ev_s = sente.feed(ack_g).unwrap();
        assert_eq!(
            ev_s,
            SessionEvent::TurnComplete { sente: Action::Resign, gote: mv("3c3d") }
        );
    }

    /// 盤面ハッシュ不一致: peer reveal の board_hash が異なると Protocol エラー。
    #[test]
    fn board_hash_mismatch_rejected() {
        let (mut sente, mut gote) = pair();
        handshake(&mut sente, &mut gote);

        let bh = initial_hash();
        let commit_s = sente.commit(bh, mv("7g7f"), Nonce([1u8; 32])).unwrap();
        let commit_g = gote.commit(bh, mv("3c3d"), Nonce([2u8; 32])).unwrap();
        gote.feed(commit_s).unwrap();
        sente.feed(commit_g).unwrap();

        // sente の reveal を改竄した board_hash で gote に届ける。
        let tampered = WireMessage::Reveal {
            action: "7g7f".to_string(),
            nonce: crate::wire::to_hex(&[1u8; 32]),
            board_hash: crate::wire::to_hex(&[0u8; 32]),
        };
        let result = gote.feed(tampered);
        assert_eq!(
            result,
            Err(SessionError::Protocol(ProtocolError::BoardHashMismatch))
        );
    }

    /// 再接続照合: 正しい auth_hash なら PeerReconnectRequest、改竄は IdentityMismatch。
    /// 承認応答 reconnect_ack_msg を feed すると ReconnectAck が返る。
    #[test]
    fn reconnect_identity_check() {
        let (mut sente, mut gote) = pair();
        handshake(&mut sente, &mut gote);

        let bh = initial_hash();
        let reconnect_s = sente.reconnect_msg(bh);
        let ev = gote.feed(reconnect_s).unwrap();
        assert_eq!(ev, SessionEvent::PeerReconnectRequest { board_hash: bh });

        let forged = WireMessage::Reconnect {
            auth_hash: crate::wire::to_hex(&[0xffu8; 32]),
            board_hash: crate::wire::to_hex(&bh.0),
        };
        let result = gote.feed(forged);
        assert_eq!(result, Err(SessionError::IdentityMismatch));

        let ack_msg = gote.reconnect_ack_msg(bh);
        let ev = sente.feed(ack_msg).unwrap();
        assert_eq!(ev, SessionEvent::ReconnectAck { resume_hash: bh });
    }

    /// handshake 前の commit は HandshakeNotDone。
    #[test]
    fn commit_before_handshake_rejected() {
        let mut sente = ClientSession::new(Side::Sente, b"shared_secret");
        let result = sente.commit(initial_hash(), mv("7g7f"), Nonce([1u8; 32]));
        assert_eq!(result, Err(SessionError::HandshakeNotDone));
    }
}
