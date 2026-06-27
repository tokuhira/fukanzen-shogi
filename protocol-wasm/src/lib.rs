use wasm_bindgen::prelude::*;

use engine::serialize::sfen_to_position;
use engine::types::{Action, Side};
use protocol::{
    auth::{hash_secret, SecretHash},
    commit::{Commitment, Nonce},
    hash::{board_hash, BoardHash},
    negotiate::{negotiate_versions, PeerVersionResponse, VersionTuple, MY_VERSION},
    session::TurnSession,
};

// ── バイト列変換ヘルパー ──────────────────────────────────────────────────────

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn from_hex32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

fn random_nonce() -> Nonce {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).expect("getrandom failed");
    Nonce(bytes)
}

// ── ユーティリティ（セッション外） ───────────────────────────────────────────

/// SFEN 文字列から盤面ハッシュ（hex 文字列）を計算する。
/// 再接続時のハッシュ照合に使う。
#[wasm_bindgen]
pub fn sfen_hash(sfen: &str) -> String {
    match sfen_to_position(sfen) {
        Some(pos) => to_hex(&board_hash(&pos).0),
        None => String::new(),
    }
}

// ── セッション ────────────────────────────────────────────────────────────────

/// ブラウザ手元で動く秘匿対戦プロトコルの状態機械。
///
/// WebSocket の送受信は JS の殻が担う。このクラスは
/// 「届いたメッセージを feed() に渡すと状態が進み、次に送るべき
/// メッセージが返る」という純粋ロジックだけを保持する。
#[wasm_bindgen]
pub struct ProtocolSession {
    side: Side,
    my_auth_hash: SecretHash,
    /// 初回 hello で相手から受け取った auth_hash（再接続の本人確認に使う）
    peer_auth_hash_hex: Option<String>,
    handshake_done: bool,
    turn: Option<TurnSession>,
    /// peer commit が自分の commit より先に届いた場合のバッファ
    pending_peer_commit: Option<Commitment>,
}

#[wasm_bindgen]
impl ProtocolSession {
    /// セッションを生成する。
    /// - `side`: `"sente"` または `"gote"`
    /// - `secret`: 対戦相手と共有するパスワード
    #[wasm_bindgen(constructor)]
    pub fn new(side: &str, secret: &str) -> ProtocolSession {
        let s = if side == "sente" { Side::Sente } else { Side::Gote };
        let my_auth_hash = hash_secret(secret.as_bytes());
        ProtocolSession {
            side: s,
            my_auth_hash,
            peer_auth_hash_hex: None,
            handshake_done: false,
            turn: None,
            pending_peer_commit: None,
        }
    }

    /// 接続直後に相手へ送る hello メッセージ（JSON 文字列）を返す。
    /// バージョン情報・認証ハッシュ・陣営を含む。
    pub fn hello_msg(&self) -> String {
        let side_str = match self.side {
            Side::Sente => "sente",
            Side::Gote => "gote",
        };
        format!(
            r#"{{"type":"hello","rule_major":{},"rule_minor":{},"proto_ver":{},"auth_hash":"{}","side":"{}"}}"#,
            MY_VERSION.rule.0,
            MY_VERSION.rule.1,
            MY_VERSION.protocol,
            to_hex(&self.my_auth_hash.0),
            side_str
        )
    }

    /// 再接続時に相手へ送るメッセージ（JSON 文字列）を返す。
    /// - `board_hash_hex`: 現在局面の盤面ハッシュ（sfen_hash() で計算）
    pub fn reconnect_msg(&self, board_hash_hex: &str) -> String {
        format!(
            r#"{{"ok":true,"message":{{"type":"reconnect","auth_hash":"{}","board_hash":"{}"}}}}"#,
            to_hex(&self.my_auth_hash.0),
            board_hash_hex
        )
    }

    /// 初回 hello で受け取った相手の auth_hash（hex）を返す。
    /// 再接続時の本人確認に使う。未取得の場合は空文字列。
    pub fn peer_auth_hash(&self) -> String {
        self.peer_auth_hash_hex.clone().unwrap_or_default()
    }

    /// 相手から届いたメッセージを処理し、状態変化を JSON で返す。
    ///
    /// 返り値の形式:
    /// - `{"ok":true,"event":"handshake_done","peer_side":"gote"}`
    /// - `{"ok":true,"event":"peer_committed","both_committed":true}`
    /// - `{"ok":true,"event":"peer_revealed","both_revealed":true}`
    /// - `{"ok":true,"event":"turn_complete","sente_usi":"7g7f","gote_usi":"3c3d"}`
    /// - `{"ok":true,"event":"peer_reconnect_request","auth_hash":"...","board_hash":"..."}`
    /// - `{"ok":true,"event":"reconnect_ack","resume_hash":"..."}`
    /// - `{"ok":false,"error":"..."}`
    pub fn feed(&mut self, msg: &str) -> String {
        let v: serde_json::Value = match serde_json::from_str(msg) {
            Ok(v) => v,
            Err(_) => return r#"{"ok":false,"error":"invalid_json"}"#.to_string(),
        };
        match v["type"].as_str() {
            Some("hello")        => self.feed_hello(&v),
            Some("commit")       => self.feed_commit(&v),
            Some("reveal")       => self.feed_reveal(&v),
            Some("ack")          => self.feed_ack(),
            Some("reconnect")    => self.feed_reconnect(&v),
            Some("reconnect_ack")=> self.feed_reconnect_ack(&v),
            Some("abort") => {
                let reason = v["reason"].as_str().unwrap_or("unknown");
                format!(r#"{{"ok":true,"event":"peer_aborted","reason":"{}"}}"#, reason)
            }
            // DO が送るシステムメッセージは JS 側が先にフィルタする想定だが念のため
            Some("peer_joined") | Some("peer_disconnected") | Some("room_ready")
            | Some("you_reconnected") | Some("peer_reconnected") => {
                format!(r#"{{"ok":true,"event":"{}"}}"#, v["type"].as_str().unwrap())
            }
            _ => r#"{"ok":false,"error":"unknown_message_type"}"#.to_string(),
        }
    }

    /// 自分の着手を確定し commit を生成する。返り値に送るべき commit JSON を含む。
    ///
    /// 返り値: `{"ok":true,"message":{...},"both_committed":false}`
    /// `both_committed` が true なら直ちに reveal_msg() を呼んでよい。
    pub fn commit_move(&mut self, sfen: &str, usi: &str) -> String {
        if !self.handshake_done {
            return r#"{"ok":false,"error":"handshake_not_done"}"#.to_string();
        }
        let action = match Action::from_usi(usi) {
            Some(a) => a,
            None => return r#"{"ok":false,"error":"invalid_usi"}"#.to_string(),
        };
        let pos = match sfen_to_position(sfen) {
            Some(p) => p,
            None => return r#"{"ok":false,"error":"invalid_sfen"}"#.to_string(),
        };
        let bh = board_hash(&pos);
        let nonce = random_nonce();

        let mut t = TurnSession::new(self.side, bh);
        let commitment = match t.local_commit(action, nonce) {
            Ok(c) => c,
            Err(e) => return format!(r#"{{"ok":false,"error":"{:?}"}}"#, e),
        };

        // バッファ済みの peer commit があれば適用する
        if let Some(pc) = self.pending_peer_commit.take() {
            let _ = t.receive_peer_commit(pc);
        }

        let both = t.both_committed();
        self.turn = Some(t);

        let inner = format!(r#"{{"type":"commit","commitment":"{}"}}"#, to_hex(&commitment.0));
        format!(r#"{{"ok":true,"message":{},"both_committed":{}}}"#, inner, both)
    }

    /// 両者 commit 後に reveal メッセージを生成する。返り値に送るべき reveal JSON を含む。
    pub fn reveal_msg(&mut self) -> String {
        let t = match self.turn.as_mut() {
            Some(t) => t,
            None => return r#"{"ok":false,"error":"no_active_turn"}"#.to_string(),
        };
        match t.local_reveal() {
            Ok(reveal) => {
                let inner = format!(
                    r#"{{"type":"reveal","action":"{}","nonce":"{}","board_hash":"{}"}}"#,
                    reveal.action.to_usi(),
                    to_hex(&reveal.nonce.0),
                    to_hex(&reveal.board_hash.0)
                );
                format!(r#"{{"ok":true,"message":{}}}"#, inner)
            }
            Err(e) => format!(r#"{{"ok":false,"error":"{:?}"}}"#, e),
        }
    }

    /// peer reveal の検証後に ack メッセージを生成する。
    pub fn ack_msg(&mut self) -> String {
        let t = match self.turn.as_mut() {
            Some(t) => t,
            None => return r#"{"ok":false,"error":"no_active_turn"}"#.to_string(),
        };
        match t.local_ack() {
            Ok(()) => r#"{"ok":true,"message":{"type":"ack"}}"#.to_string(),
            Err(e) => format!(r#"{{"ok":false,"error":"{:?}"}}"#, e),
        }
    }
}

// ── feed の内部ハンドラ ───────────────────────────────────────────────────────

impl ProtocolSession {
    fn feed_hello(&mut self, v: &serde_json::Value) -> String {
        if self.handshake_done {
            return r#"{"ok":false,"error":"duplicate_hello"}"#.to_string();
        }

        // バージョン交渉
        let rule_major = v["rule_major"].as_u64().unwrap_or(u64::MAX) as u32;
        let rule_minor = v["rule_minor"].as_u64().unwrap_or(u64::MAX) as u32;
        let proto_ver = v["proto_ver"].as_u64().unwrap_or(u64::MAX) as u32;
        let peer_ver = VersionTuple {
            rule: (rule_major, rule_minor),
            protocol: proto_ver,
        };
        if let Err(e) = negotiate_versions(&MY_VERSION, PeerVersionResponse::Version(peer_ver)) {
            return format!(r#"{{"ok":false,"error":"version_mismatch","detail":"{:?}"}}"#, e);
        }

        // 本人認証: peer の auth_hash を記録（再接続時の照合に使う）
        let auth_hex = v["auth_hash"].as_str().unwrap_or("");
        if from_hex32(auth_hex).is_none() {
            return r#"{"ok":false,"error":"invalid_auth_hash"}"#.to_string();
        }
        self.peer_auth_hash_hex = Some(auth_hex.to_string());

        let peer_side = v["side"].as_str().unwrap_or("unknown");
        self.handshake_done = true;

        format!(
            r#"{{"ok":true,"event":"handshake_done","peer_side":"{}"}}"#,
            peer_side
        )
    }

    fn feed_commit(&mut self, v: &serde_json::Value) -> String {
        let hex = v["commitment"].as_str().unwrap_or("");
        let bytes = match from_hex32(hex) {
            Some(b) => b,
            None => return r#"{"ok":false,"error":"invalid_commitment"}"#.to_string(),
        };
        let commitment = Commitment(bytes);

        if let Some(ref mut t) = self.turn {
            match t.receive_peer_commit(commitment) {
                Ok(()) => {
                    let both = t.both_committed();
                    format!(r#"{{"ok":true,"event":"peer_committed","both_committed":{}}}"#, both)
                }
                Err(e) => format!(r#"{{"ok":false,"error":"{:?}"}}"#, e),
            }
        } else {
            // 自分がまだ commit していない → バッファ
            self.pending_peer_commit = Some(commitment);
            r#"{"ok":true,"event":"peer_commit_buffered"}"#.to_string()
        }
    }

    fn feed_reveal(&mut self, v: &serde_json::Value) -> String {
        let t = match self.turn.as_mut() {
            Some(t) => t,
            None => return r#"{"ok":false,"error":"no_active_turn"}"#.to_string(),
        };

        let action_str = v["action"].as_str().unwrap_or("");
        let action = match Action::from_usi(action_str) {
            Some(a) => a,
            None => return r#"{"ok":false,"error":"invalid_action"}"#.to_string(),
        };

        let nonce_bytes = match from_hex32(v["nonce"].as_str().unwrap_or("")) {
            Some(b) => b,
            None => return r#"{"ok":false,"error":"invalid_nonce"}"#.to_string(),
        };
        let hash_bytes = match from_hex32(v["board_hash"].as_str().unwrap_or("")) {
            Some(b) => b,
            None => return r#"{"ok":false,"error":"invalid_board_hash"}"#.to_string(),
        };

        match t.receive_peer_reveal(action, Nonce(nonce_bytes), BoardHash(hash_bytes)) {
            Ok(()) => {
                let both = t.both_revealed();
                format!(r#"{{"ok":true,"event":"peer_revealed","both_revealed":{}}}"#, both)
            }
            Err(e) => format!(r#"{{"ok":false,"error":"{:?}"}}"#, e),
        }
    }

    fn feed_ack(&mut self) -> String {
        let t = match self.turn.as_mut() {
            Some(t) => t,
            None => return r#"{"ok":false,"error":"no_active_turn"}"#.to_string(),
        };

        t.receive_peer_ack();

        if t.is_complete() {
            if let Some((sa, ga)) = t.get_actions() {
                let msg = format!(
                    r#"{{"ok":true,"event":"turn_complete","sente_usi":"{}","gote_usi":"{}"}}"#,
                    sa.to_usi(),
                    ga.to_usi()
                );
                // 次ターンの peer_commit を feed_commit が正しくバッファできるよう解放する
                self.turn = None;
                return msg;
            }
        }
        r#"{"ok":true,"event":"peer_acked"}"#.to_string()
    }

    /// 相手から届いた reconnect メッセージを受け取る。
    /// JS 側でハッシュ照合・本人確認を行うための情報を返す。
    fn feed_reconnect(&mut self, v: &serde_json::Value) -> String {
        let auth_hash  = v["auth_hash"].as_str().unwrap_or("");
        let board_hash = v["board_hash"].as_str().unwrap_or("");
        format!(
            r#"{{"ok":true,"event":"peer_reconnect_request","auth_hash":"{}","board_hash":"{}"}}"#,
            auth_hash,
            board_hash
        )
    }

    /// 相手から届いた reconnect_ack メッセージ（再接続承認）を受け取る。
    fn feed_reconnect_ack(&mut self, v: &serde_json::Value) -> String {
        let resume_hash = v["board_hash"].as_str().unwrap_or("");
        format!(
            r#"{{"ok":true,"event":"reconnect_ack","resume_hash":"{}"}}"#,
            resume_hash
        )
    }
}
