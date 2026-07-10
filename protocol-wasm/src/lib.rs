use wasm_bindgen::prelude::*;

use engine::serialize::sfen_to_position;
use engine::types::{Action, Side};
use protocol::client::{ClientSession, SessionError, SessionEvent};
use protocol::commit::Nonce;
use protocol::hash::{board_hash, BoardHash};
use protocol::negotiate::MY_VERSION;
use protocol::wire::{from_hex32, to_hex, WireError, WireMessage};

// ── バイト列変換ヘルパー ──────────────────────────────────────────────────────

fn random_nonce() -> Nonce {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).expect("getrandom failed");
    Nonce(bytes)
}

fn side_str(s: Side) -> &'static str {
    match s {
        Side::Sente => "sente",
        Side::Gote => "gote",
    }
}

fn err_json(code: &str) -> String {
    format!(r#"{{"ok":false,"error":"{}"}}"#, code)
}

fn wrapped_msg(m: WireMessage) -> String {
    format!(r#"{{"ok":true,"message":{}}}"#, m.to_json())
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

/// このビルドが実装するルール・プロトコル・アプリの版タプルを JSON で返す。
///
/// 返値: `{"rule":"0.5","protocol":2,"app":"0.8.0"}`
#[wasm_bindgen]
pub fn version_tuple() -> String {
    format!(
        r#"{{"rule":"{}.{}","protocol":{},"app":"{}"}}"#,
        MY_VERSION.rule.0,
        MY_VERSION.rule.1,
        MY_VERSION.protocol,
        env!("CARGO_PKG_VERSION")
    )
}

// ── セッション ────────────────────────────────────────────────────────────────

/// ブラウザ手元で動く秘匿対戦プロトコルの状態機械。
///
/// WebSocket の送受信は JS の殻が担う。このクラスは `protocol::ClientSession`
/// の薄い wasm_bindgen ラッパ——sfen/usi の解釈と nonce 生成を担い、
/// JS 向けの JSON 文字列契約へ整形するだけ。
#[wasm_bindgen]
pub struct ProtocolSession {
    inner: ClientSession,
}

#[wasm_bindgen]
impl ProtocolSession {
    /// セッションを生成する。
    /// - `side`: `"sente"` または `"gote"`
    /// - `secret`: 対戦相手と共有するパスワード
    #[wasm_bindgen(constructor)]
    pub fn new(side: &str, secret: &str) -> ProtocolSession {
        let s = if side == "sente" {
            Side::Sente
        } else {
            Side::Gote
        };
        ProtocolSession {
            inner: ClientSession::new(s, secret.as_bytes()),
        }
    }

    /// 接続直後に相手へ送る hello メッセージ（JSON 文字列）を返す。
    /// バージョン情報・認証ハッシュ・陣営を含む。裸の JSON（online.js は parse せず
    /// そのまま送る）。
    pub fn hello_msg(&self) -> String {
        self.inner.hello_msg().to_json()
    }

    /// 再接続時に相手へ送るメッセージ（JSON 文字列）を返す。
    /// - `board_hash_hex`: 現在局面の盤面ハッシュ（sfen_hash() で計算）
    pub fn reconnect_msg(&self, board_hash_hex: &str) -> String {
        match from_hex32(board_hash_hex) {
            Some(b) => wrapped_msg(self.inner.reconnect_msg(BoardHash(b))),
            None => err_json("invalid_board_hash"),
        }
    }

    /// 初回 hello で受け取った相手の auth_hash（hex）を返す。
    /// 2a（再接続の本人照合を核へ引き上げ）後は online.js から呼ばれないが、
    /// 契約保存のため薄い鏡として残す。
    pub fn peer_auth_hash(&self) -> String {
        self.inner
            .peer_auth_hash()
            .map(|h| to_hex(&h.0))
            .unwrap_or_default()
    }

    /// 自分の着手を確定し commit を生成する。返り値に送るべき commit JSON を含む。
    ///
    /// 返り値: `{"ok":true,"message":{...},"both_committed":false}`
    /// `both_committed` が true なら直ちに reveal_msg() を呼んでよい。
    pub fn commit_move(&mut self, sfen: &str, usi: &str) -> String {
        let action = match Action::from_usi(usi) {
            Some(a) => a,
            None => return err_json("invalid_usi"),
        };
        let pos = match sfen_to_position(sfen) {
            Some(p) => p,
            None => return err_json("invalid_sfen"),
        };
        let bh = board_hash(&pos);
        let nonce = random_nonce();
        match self.inner.commit(bh, action, nonce) {
            Ok(msg) => format!(
                r#"{{"ok":true,"message":{},"both_committed":{}}}"#,
                msg.to_json(),
                self.inner.both_committed()
            ),
            Err(e) => err_json(&session_error_code(&e)),
        }
    }

    /// 両者 commit 後に reveal メッセージを生成する。返り値に送るべき reveal JSON を含む。
    pub fn reveal_msg(&mut self) -> String {
        match self.inner.reveal_msg() {
            Ok(m) => wrapped_msg(m),
            Err(e) => err_json(&session_error_code(&e)),
        }
    }

    /// peer reveal の検証後に ack メッセージを生成する。
    pub fn ack_msg(&mut self) -> String {
        match self.inner.ack_msg() {
            Ok(m) => wrapped_msg(m),
            Err(e) => err_json(&session_error_code(&e)),
        }
    }

    /// 相手から届いたメッセージを処理し、状態変化を JSON で返す。
    ///
    /// 返り値の形式:
    /// - `{"ok":true,"event":"handshake_done","peer_side":"gote"}`
    /// - `{"ok":true,"event":"peer_committed","both_committed":true}`
    /// - `{"ok":true,"event":"peer_revealed","both_revealed":true}`
    /// - `{"ok":true,"event":"turn_complete","sente_usi":"7g7f","gote_usi":"3c3d"}`
    /// - `{"ok":true,"event":"peer_reconnect_request","board_hash":"..."}`
    /// - `{"ok":true,"event":"peer_reconnect_rejected","reason":"auth_mismatch"}`
    /// - `{"ok":true,"event":"reconnect_ack","resume_hash":"..."}`
    /// - `{"ok":false,"error":"..."}`
    pub fn feed(&mut self, msg: &str) -> String {
        let wire = match WireMessage::from_json(msg) {
            Ok(w) => w,
            Err(WireError::InvalidJson) => return err_json("invalid_json"),
            // DO システムメッセージは online.js が feed 前にフィルタ済み。
            // ここへ来る未知 type は旧同様 unknown_message_type（安全弁）。
            Err(WireError::UnknownType) => return err_json("unknown_message_type"),
        };
        match self.inner.feed(wire) {
            Ok(ev) => event_json(ev),
            // 版不一致は detail 付き（旧 feed_hello と一致）。
            Err(SessionError::VersionMismatch(o)) => {
                format!(
                    r#"{{"ok":false,"error":"version_mismatch","detail":"{:?}"}}"#,
                    o
                )
            }
            // IdentityMismatch は feed(Reconnect) からのみ到達（2a）。
            // ok:false にすると online.js の汎用エラー経路で ws.close されてしまい、
            // 相手へ abort を送る礼儀が失われる。よって「ok:true の拒否イベント」で返し、
            // online.js が abort を送れるようにする。
            Err(SessionError::IdentityMismatch) => {
                r#"{"ok":true,"event":"peer_reconnect_rejected","reason":"auth_mismatch"}"#
                    .to_string()
            }
            Err(e) => err_json(&session_error_code(&e)),
        }
    }
}

// ── イベント・エラーの整形 ────────────────────────────────────────────────────

fn event_json(ev: SessionEvent) -> String {
    match ev {
        SessionEvent::HandshakeDone { peer_side } => format!(
            r#"{{"ok":true,"event":"handshake_done","peer_side":"{}"}}"#,
            side_str(peer_side)
        ),
        SessionEvent::PeerCommitted { both_committed } => format!(
            r#"{{"ok":true,"event":"peer_committed","both_committed":{}}}"#,
            both_committed
        ),
        SessionEvent::PeerCommitBuffered => {
            r#"{"ok":true,"event":"peer_commit_buffered"}"#.to_string()
        }
        SessionEvent::PeerRevealed { both_revealed } => format!(
            r#"{{"ok":true,"event":"peer_revealed","both_revealed":{}}}"#,
            both_revealed
        ),
        SessionEvent::PeerAcked => r#"{"ok":true,"event":"peer_acked"}"#.to_string(),
        SessionEvent::TurnComplete { sente, gote } => format!(
            r#"{{"ok":true,"event":"turn_complete","sente_usi":"{}","gote_usi":"{}"}}"#,
            sente.to_usi(),
            gote.to_usi()
        ),
        // 2a: auth_hash は載せない（照合は核が済ませた）。board_hash のみ。
        SessionEvent::PeerReconnectRequest { board_hash } => format!(
            r#"{{"ok":true,"event":"peer_reconnect_request","board_hash":"{}"}}"#,
            to_hex(&board_hash.0)
        ),
        SessionEvent::ReconnectAck { resume_hash } => format!(
            r#"{{"ok":true,"event":"reconnect_ack","resume_hash":"{}"}}"#,
            to_hex(&resume_hash.0)
        ),
        SessionEvent::PeerAborted { reason } => {
            format!(
                r#"{{"ok":true,"event":"peer_aborted","reason":"{}"}}"#,
                reason
            )
        }
    }
}

fn session_error_code(e: &SessionError) -> String {
    match e {
        SessionError::DuplicateHello => "duplicate_hello".to_string(),
        SessionError::HandshakeNotDone => "handshake_not_done".to_string(),
        SessionError::NoActiveTurn => "no_active_turn".to_string(),
        SessionError::Protocol(pe) => format!("{:?}", pe), // 旧 feed も {:?} で返していた
        SessionError::BadHex => "invalid_hex".to_string(),
        SessionError::InvalidUsi => "invalid_action".to_string(), // feed(Reveal) 文脈
        // VersionMismatch / IdentityMismatch は feed() で特別扱い（ここには来ない）
        SessionError::VersionMismatch(_) | SessionError::IdentityMismatch => {
            "unexpected".to_string()
        }
    }
}
