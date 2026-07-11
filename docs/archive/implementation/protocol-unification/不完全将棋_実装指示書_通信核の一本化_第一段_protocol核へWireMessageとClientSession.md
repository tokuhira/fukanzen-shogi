# 不完全将棋 実装指示書 — 通信核の一本化 第一段：`protocol` 核へ `WireMessage` と `ClientSession`（PROTOCOL 5・要石）

> 対象実行者: Claude Code（Sonnet 5）
> 前提: 配布 v0.11.2 / web `?v=`0.11.11 / ルール v0.6 / PROTOCOL 4。board.js 分割アーク完了。`protocol` クレートに `session::TurnSession`（純粋 commit-reveal 状態機械）が既にあり、web（protocol-wasm）も TUI（online.rs）も共有している。この段は **`protocol` クレートへの純粋な追加のみ**——他クレートは一切変更しない。`cargo test -p protocol` で完結する。
> 関連する現物（すべて実地で確認済み・行番号は現 HEAD 基準）:
> - `protocol/src/session.rs`: `TurnSession`（`new(local_side, current_pos_hash)` / `local_commit(action, nonce)->Commitment` / `receive_peer_commit(Commitment)` / `both_committed()` / `local_reveal()->Reveal` / `receive_peer_reveal(action, nonce, board_hash)` / `both_revealed()` / `local_ack()` / `receive_peer_ack()` / `is_complete()` / `get_actions()->Option<(Action,Action)>`）。`Reveal{action, nonce, board_hash}`。`ProtocolError`（8 variant）。**乱数 Nonce は呼び出し側が渡す**（決定的テスト）。
> - `protocol/src/lib.rs`: 再エクスポート `hash_secret, verify_secret, SecretHash` / `make_commit, verify_commit, Commitment, Nonce` / `board_hash, BoardHash` / `negotiate_versions, NegotiationOutcome, PeerVersionResponse, VersionCleared, VersionTuple, MY_VERSION, PROTOCOL_VERSION` / `RecoverySession` / `ProtocolError, Reveal, TurnSession`。
> - `protocol/src/negotiate.rs`: `PROTOCOL_VERSION`（現 4）と `MY_VERSION: VersionTuple{rule:(u32,u32), protocol:u32}`。`negotiate_versions(&mine, PeerVersionResponse::Version(peer)) -> Result<VersionCleared, NegotiationOutcome>`。
> - `protocol/src/auth.rs`: `hash_secret(&[u8]) -> SecretHash`（内部 `SecretHash([u8;32])`）。
> - **正本の写し元**: `protocol-wasm/src/lib.rs` の `ProtocolSession`。この段が抜き出す orchestration の**挙動の一字一句の出典**。特に `feed_hello`（版交渉＋peer_auth_hash 記録）・`feed_commit`（先着バッファ `pending_peer_commit`）・`feed_reveal`・`feed_ack`（`turn_complete` 時に `self.turn=None` で解放）・`commit_move`（新 `TurnSession` を張り、バッファ済み peer commit を適用）・`reveal_msg`・`ack_msg`・`feed_reconnect`・`feed_reconnect_ack`。この段はこれらを **typed な純粋 Rust** として `protocol` 核へ移す（wasm の衣を脱がせる）。
> - `engine::types::{Action, Side}`。`Action::from_usi(&str)->Option<Action>` / `Action::to_usi()->String`。`Side::{Sente, Gote}`。
> 関連文書: `不完全将棋_実装指示書_通信核の一本化アーク_概観と段組`（このアークの錨・四層・canonical 語彙・版の物語）、`design/不完全将棋_版図_世界観と設計方針`、`design/不完全将棋_バージョン互換性管理_方針`。
> 性格: 第一段は**「対局チャネルのワイヤ語彙（層 B・`WireMessage`）と、セッション進行の orchestration（層 C・`ClientSession`）を `protocol` 核へ抜き、PROTOCOL を 5 へ上げる」**。純粋 Rust の追加のみ。transport 非依存・wasm 非依存・TUI 非依存。挙動の出典は protocol-wasm の `ProtocolSession`——**同じ状態遷移を typed で再現**する（web の挙動を第二段で保存するための土台）。`WireMessage` は**対局チャネル（"other_player_only"）の語彙に限る**——DO のシステム/部屋メッセージは含めない。この段は他クレートを触らない・wasm 再ビルドなし・利用者に見える変化なし。

---

## 0. 目的と範囲

- **作るもの**:
  1. `protocol/src/wire.rs` — 対局チャネルのワイヤ語彙 `WireMessage`（serde タグ付き enum）。JSON の唯一の正本。`to_json()->String` / `from_json(&str)->Result<WireMessage, WireError>`。hex ⇄ バイト列のヘルパはここに集約（net.rs や protocol-wasm の重複を将来ここへ寄せる下地）。
  2. `protocol/src/client.rs` — セッション orchestration `ClientSession`（純粋・transport 非依存）。`new(side, secret)` / `hello_msg()` / `reconnect_msg(board_hash)` / `reconnect_ack_msg(resume_hash)` / `feed(WireMessage)->Result<SessionEvent, SessionError>` / `commit(board_hash, action, nonce)->Result<WireMessage, SessionError>` / `both_committed()` / `reveal_msg()->Result<WireMessage, SessionError>` / `ack_msg()->Result<WireMessage, SessionError>` / `peer_auth_hash()->Option<SecretHash>` / `handshake_done()->bool`。
  3. `protocol/src/lib.rs` — `pub mod wire; pub mod client;` と、`WireMessage`・`WireError`・`ClientSession`・`SessionEvent`・`SessionError` の再エクスポート。
  4. `PROTOCOL_VERSION` を **4→5** へ（`negotiate.rs`）。
  5. 単体テスト（`wire.rs`・`client.rs` の `#[cfg(test)]`）——ワイヤの往復・二者の完全な一局・先着バッファ・版不一致・再接続照合。**すべて `cargo test -p protocol` で緑**。
- **位置づけ**: 通信核の一本化アークの**第一段（要石）**。他の三段（web ラッパ痩せ・TUI ネイティブ化・TUI クラウド殻）が参照する canonical な核を据える。
- **作らないもの（＝理由つき）**:
  - **protocol-wasm・TUI・web・server の変更**: この段は `protocol` への追加のみ。既存の `NetMessage`（net.rs）も `ProtocolSession`（protocol-wasm）も**そのまま残す**（第二・三段で置換）。並存しても衝突しない（別クレート・別型）。
  - **DO のシステム/部屋メッセージの型**（peer_joined 等）: 対局チャネルの語彙ではない。層 D（WS 殻・第四段）の関心事。`WireMessage` に含めない。
  - **再接続の再開点探索**（board_hash を kifu 履歴から探す `RecoverySession` 相当）: 対局履歴を要する＝殻／呼び出し側の責務。`ClientSession` は**本人照合（auth_hash 一致）とワイヤ組み立てまで**を担い、再開点の確認は殻に委ねる（`feed` は board_hash を surface するだけ）。
  - **nonce の生成**: 呼び出し側が渡す（`TurnSession` の流儀を継ぐ・決定的テスト）。`ClientSession::commit` は `nonce: Nonce` を引数で受ける。
  - **sfen→board_hash・usi→Action の解釈**: 呼び出し側／殻で行い、`ClientSession` は typed（`BoardHash`・`Action`）で受ける。核は `engine::serialize`（sfen 解釈）に依存しない。

---

## 1. `protocol/src/wire.rs`（対局チャネルの語彙・serde 正本）

`WireMessage` は概観 §3 の canonical 語彙。serde の内部タグで `type` を出す。フィールド名・型は下記を**一字一句**守る（web が第二段でこのバイト列を保存するため）。

```rust
//! 対局チャネル（DO の routeDecision で言う "other_player_only"）のワイヤ語彙。
//! JSON の唯一の正本。hello / commit / reveal / ack / reconnect / reconnect_ack / abort。
//! DO のシステム・部屋メッセージ（peer_joined 等）は含めない——それは層D（殻）の関心事。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WireMessage {
    /// 接続直後のハンドシェイク。版＋認証ハッシュ＋陣営を一通に集約。
    Hello {
        rule_major: u32,
        rule_minor: u32,
        proto_ver: u32,
        auth_hash: String,  // hex(SHA-256(secret))
        side: String,       // "sente" | "gote"
    },
    /// commit フェーズ。
    Commit { commitment: String },      // hex, 32byte
    /// reveal フェーズ。着手欄は `action`（USI 文字列）。
    Reveal {
        action: String,      // USI
        nonce: String,       // hex, 32byte
        board_hash: String,  // hex, 32byte
    },
    /// ack フェーズ。
    Ack,
    /// 再接続ハンドシェイク。生 secret は晒さず auth_hash を送る。
    Reconnect {
        auth_hash: String,   // hex
        board_hash: String,  // hex（現局面）
    },
    /// 再接続の承認応答（再開点の board_hash）。
    ReconnectAck { board_hash: String },  // hex
    /// プロトコル違反・版不一致・認証失敗によるアボート。
    Abort { reason: String },
}
```

- 往復ヘルパ:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireError {
    InvalidJson,
    UnknownType,       // 対局チャネル外の type（DO システムメッセージ等が誤って渡った）
}

impl WireMessage {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("WireMessage serialize は無謬")
    }
    /// 対局チャネルの WireMessage を厳密に解釈する。
    /// 未知 type（peer_joined 等 DO システムメッセージを含む）は `UnknownType`。
    pub fn from_json(s: &str) -> Result<WireMessage, WireError> {
        // まず妥当な JSON か、次に既知 type か。serde の untagged 失敗と
        // 「JSON 不正」を区別するため、一度 Value で type を覗いてから分岐してもよいが、
        // 最小実装は serde_json::from_str::<WireMessage> の Err を UnknownType/InvalidJson へ割る。
        match serde_json::from_str::<WireMessage>(s) {
            Ok(m) => Ok(m),
            Err(_) => {
                // JSON として妥当かを確かめ、妥当なら「未知 type」、不正なら InvalidJson。
                if serde_json::from_str::<serde_json::Value>(s).is_ok() {
                    Err(WireError::UnknownType)
                } else {
                    Err(WireError::InvalidJson)
                }
            }
        }
    }
}
```

- **hex ⇄ バイト列ヘルパ**: net.rs（`to_hex`/`from_hex`/`commitment_from_hex`/`nonce_from_hex`/`board_hash_from_hex`）と protocol-wasm（`to_hex`/`from_hex32`）に同型の重複がある。この段で `wire.rs` に **`to_hex(&[u8])->String` と `from_hex32(&str)->Option<[u8;32]>`** を pub で据え、以後の段が寄せられる正本にする（この段では他クレートを触らないので、寄せは第二・三段で行う。ここは新設だけ）。

**受け入れ**: 全 7 variant で `to_json` → `from_json` の往復が元と一致。`from_json("{\"type\":\"peer_joined\"}")` が `Err(WireError::UnknownType)`。`from_json("not json")` が `Err(WireError::InvalidJson)`。バイト列: `Reveal` が `{"type":"reveal","action":"7g7f","nonce":"...","board_hash":"..."}`、`Hello` の欄が `proto_ver`/`auth_hash`/`side`、`ReconnectAck` が `{"type":"reconnect_ack","board_hash":"..."}` になること（protocol-wasm の現行バイト列と一致）。

## 2. `protocol/src/client.rs`（セッション orchestration・純粋・transport 非依存）

`ProtocolSession`（protocol-wasm）の状態遷移を typed で再現する。**wasm の String 入出力を、typed な `WireMessage` 入出力に置き換えた**もの。挙動は出典（protocol-wasm）に忠実に。

```rust
use engine::types::{Action, Side};
use crate::auth::{hash_secret, SecretHash};
use crate::commit::{Commitment, Nonce};
use crate::hash::BoardHash;
use crate::negotiate::{negotiate_versions, MY_VERSION, NegotiationOutcome, PeerVersionResponse, VersionTuple};
use crate::session::{ProtocolError, TurnSession};
use crate::wire::WireMessage;

pub struct ClientSession {
    side: Side,
    my_auth_hash: SecretHash,
    peer_auth_hash: Option<SecretHash>,  // 初回 hello で記録（再接続照合に使う）
    handshake_done: bool,
    turn: Option<TurnSession>,
    pending_peer_commit: Option<Commitment>,  // 自分の commit より先に届いた peer commit
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
    IdentityMismatch,     // 再接続 auth_hash が初回 peer_auth_hash と不一致
    BadHex,               // hex 欄が不正
    InvalidUsi,           // action 欄が USI として不正
    UnexpectedMessage,    // このフェーズで来ない type
}
```

### 2.1 構築・ハンドシェイク組み立て

```rust
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

    pub fn handshake_done(&self) -> bool { self.handshake_done }
    pub fn peer_auth_hash(&self) -> Option<SecretHash> { self.peer_auth_hash }

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
        WireMessage::ReconnectAck { board_hash: crate::wire::to_hex(&resume_hash.0) }
    }
}

fn side_str(s: Side) -> &'static str {
    match s { Side::Sente => "sente", Side::Gote => "gote" }
}
fn parse_side(s: &str) -> Side {
    if s == "sente" { Side::Sente } else { Side::Gote }
}
```

### 2.2 `commit` / `reveal_msg` / `ack_msg`（出典: protocol-wasm `commit_move`/`reveal_msg`/`ack_msg`）

```rust
impl ClientSession {
    /// 自分の着手を確定し commit を生成。新 TurnSession を張り、
    /// 先着していた peer commit があれば適用する（出典: commit_move）。
    /// board_hash・action・nonce は呼び出し側が用意（核は sfen/usi 解釈・乱数を持たない）。
    pub fn commit(&mut self, board_hash: BoardHash, action: Action, nonce: Nonce)
        -> Result<WireMessage, SessionError>
    {
        if !self.handshake_done { return Err(SessionError::HandshakeNotDone); }
        let mut t = TurnSession::new(self.side, board_hash);
        let commitment = t.local_commit(action, nonce).map_err(SessionError::Protocol)?;
        if let Some(pc) = self.pending_peer_commit.take() {
            // 先着 peer commit。二重 commit 等のエラーは握り潰さず…ではなく、
            // 出典（commit_move）は `let _ =` で無視している。挙動保存のため無視する。
            let _ = t.receive_peer_commit(pc);
        }
        self.turn = Some(t);
        Ok(WireMessage::Commit { commitment: crate::wire::to_hex(&commitment.0) })
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
}
```

**挙動保存の急所（出典と一致させる）**:
- `commit`: `pending_peer_commit` が先着していたら適用。`receive_peer_commit` のエラーは**握り潰す**（出典 `commit_move` の `let _ =`）。`both_committed()` が真かは呼び出し側が `both_committed()` で見て `reveal_msg()` を呼ぶ（出典は `both_committed` を返り値で伝えていた——ここでは getter で代替）。
- `commit` は handshake 前ならエラー（出典: `handshake_not_done`）。

### 2.3 `feed`（出典: protocol-wasm `feed` とその内部ハンドラ群）

```rust
impl ClientSession {
    pub fn feed(&mut self, msg: WireMessage) -> Result<SessionEvent, SessionError> {
        match msg {
            WireMessage::Hello { rule_major, rule_minor, proto_ver, auth_hash, side } => {
                if self.handshake_done { return Err(SessionError::DuplicateHello); }
                let peer = VersionTuple { rule: (rule_major, rule_minor), protocol: proto_ver };
                negotiate_versions(&MY_VERSION, PeerVersionResponse::Version(peer))
                    .map_err(SessionError::VersionMismatch)?;
                let ah = parse_hash(&auth_hash)?;      // 不正 hex は BadHex
                self.peer_auth_hash = Some(ah);
                self.handshake_done = true;
                Ok(SessionEvent::HandshakeDone { peer_side: parse_side(&side) })
            }
            WireMessage::Commit { commitment } => {
                let c = Commitment(parse_bytes32(&commitment)?);
                if let Some(t) = self.turn.as_mut() {
                    t.receive_peer_commit(c).map_err(SessionError::Protocol)?;
                    Ok(SessionEvent::PeerCommitted { both_committed: t.both_committed() })
                } else {
                    self.pending_peer_commit = Some(c);        // 先着バッファ
                    Ok(SessionEvent::PeerCommitBuffered)
                }
            }
            WireMessage::Reveal { action, nonce, board_hash } => {
                let t = self.turn.as_mut().ok_or(SessionError::NoActiveTurn)?;
                let a = Action::from_usi(&action).ok_or(SessionError::InvalidUsi)?;
                let n = Nonce(parse_bytes32(&nonce)?);
                let bh = BoardHash(parse_bytes32(&board_hash)?);
                t.receive_peer_reveal(a, n, bh).map_err(SessionError::Protocol)?;
                Ok(SessionEvent::PeerRevealed { both_revealed: t.both_revealed() })
            }
            WireMessage::Ack => {
                let t = self.turn.as_mut().ok_or(SessionError::NoActiveTurn)?;
                t.receive_peer_ack();
                if t.is_complete() {
                    if let Some((sente, gote)) = t.get_actions() {
                        self.turn = None;   // 次ターンの先着 commit を feed が正しくバッファできるよう解放（出典と一致）
                        return Ok(SessionEvent::TurnComplete { sente, gote });
                    }
                }
                Ok(SessionEvent::PeerAcked)
            }
            WireMessage::Reconnect { auth_hash, board_hash } => {
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

fn parse_bytes32(hex: &str) -> Result<[u8; 32], SessionError> {
    crate::wire::from_hex32(hex).ok_or(SessionError::BadHex)
}
fn parse_hash(hex: &str) -> Result<SecretHash, SessionError> {
    Ok(SecretHash(parse_bytes32(hex)?))
}
```

**挙動保存の急所（出典と一致させる）**:
- `feed_ack` の `turn_complete` 時に `self.turn=None` で**解放**（次ターンの先着 commit を feed が「セッション未初期化」でなく「バッファ」として受けられるようにするため）。出典のコメントどおり。
- `feed_commit` で `turn` が無いときは**バッファ**（`PeerCommitBuffered`）——エラーにしない。出典どおり。
- **再接続の再定義**（このアークの決定 3）: 出典の `feed_reconnect` は auth_hash を surface するだけで照合を JS に委ねていた。この段では**照合を核へ引き上げる**——`peer_auth_hash` との一致を `feed` 内で確認し、不一致は `IdentityMismatch`。再開点（board_hash が履歴のどこか）の確認は**殻の責務**（`PeerReconnectRequest{board_hash}` を surface するにとどめる）。
- DO のシステム/部屋メッセージ（peer_joined 等）は `WireMessage` に存在しない＝そもそも feed に来ない（殻が層 D で処理して feed しない）。出典が `feed` 内で防御的にエコーしていた分岐は**持ち込まない**（境界を核で綺麗にする）。

### 2.4 `lib.rs` への配線と PROTOCOL bump

```rust
// protocol/src/lib.rs に追加
pub mod client;
pub mod wire;
pub use client::{ClientSession, SessionError, SessionEvent};
pub use wire::{WireError, WireMessage};
```

`negotiate.rs` の `PROTOCOL_VERSION` を **4 → 5**。`MY_VERSION` がこれを読むなら自動で伝播。**この段で他クレートは再ビルドしない**が、次段以降で web・TUI が再ビルドされると自動で 5 を名乗る（版の物語＝概観 §5）。

## 3. テスト（`cargo test -p protocol` で緑）

`wire.rs` `#[cfg(test)]`:
- 全 7 variant の `to_json`→`from_json` 往復一致。
- 具体バイト列の固定（`Reveal` の欄名 `action`、`Hello` の `proto_ver`/`auth_hash`/`side`、`ReconnectAck` の `{"type":"reconnect_ack","board_hash":...}`）——protocol-wasm の現行出力と照合。
- `from_json("{\"type\":\"peer_joined\"}")` → `UnknownType`。`from_json("x")` → `InvalidJson`。

`client.rs` `#[cfg(test)]`（Nonce は固定バイトで注入・決定的）:
- **完全な一局（先手視点）**: 二つの `ClientSession`（Sente/Gote・同一 secret）を作り、互いの `hello_msg` を相手に `feed` → 両者 `HandshakeDone{peer_side}`。Sente が `commit(bh, 7g7f, n1)`、Gote が `commit(bh, 3c3d, n2)`、互いの Commit を feed（`PeerCommitted{both:true}`）、`reveal_msg` を交換 feed（`PeerRevealed`）、`ack_msg` を交換 feed → 双方 `TurnComplete{sente:7g7f, gote:3c3d}`。**盤面ハッシュは両者一致**（同一 `board_hash` を渡す）。
- **先着バッファ**: 一方の Commit を相手が自分の commit 前に feed → `PeerCommitBuffered`。その後 `commit` すると先着分が適用され `both_committed()==true`。
- **版不一致**: `Hello` の proto_ver を別値にして feed → `Err(VersionMismatch(_))`。
- **投了**: `commit(bh, Action::Resign, n)` を通し、`TurnComplete` が `Resign` を含んで返る（`Action::Resign` は `TurnSession` テストに前例あり）。
- **盤面ハッシュ不一致**: peer reveal の board_hash を別値にして feed → `Err(Protocol(BoardMismatch 相当))`（`TurnSession::receive_peer_reveal` の `BoardHashMismatch`）。
- **再接続照合**: hello 交換後、一方の `reconnect_msg(bh)` を相手に feed → `PeerReconnectRequest{board_hash:bh}`。auth_hash を改竄した Reconnect を feed → `Err(IdentityMismatch)`。`reconnect_ack_msg(resume)` を feed → `ReconnectAck{resume_hash:resume}`。
- **handshake 前 commit**: `new` 直後に `commit` → `Err(HandshakeNotDone)`。

## 4. 受け入れ条件（この段の完了）

- `cargo test -p protocol` が緑（新規テスト全通過・既存 `TurnSession` テスト無傷）。
- `cargo build` がワークスペース全体で通る（`protocol` への追加が他クレートを壊さない——他クレートは無変更なので新型を使わないだけ）。
- `WireMessage` の JSON バイト列が protocol-wasm の現行出力と一致（第二段で web の挙動を保存できる土台）。
- `PROTOCOL_VERSION == 5`。
- `protocol-wasm`・`tui`・`web`・`server` に**差分ゼロ**（この段は `protocol` のみ）。
- 版タプル: 配布版・web `?v=` は**据え置き**（利用者に見える変化なし）。PROTOCOL 定数のみ 5 へ。

## 末尾要約

`protocol` に対局チャネルの語彙 `WireMessage`（serde 正本）とセッション orchestration `ClientSession`（純粋・transport 非依存）を新設し、PROTOCOL を 5 へ上げる。挙動の出典は protocol-wasm の `ProtocolSession`——同じ状態遷移（handshake の版交渉＋peer_auth_hash 記録・commit の先着バッファ・ack 完了時の turn 解放）を typed で再現し、**再接続だけは照合を核へ引き上げて再定義**（生 secret を晒さず auth_hash 一致で照合、再開点確認は殻へ委ねる）。`WireMessage` は対局チャネル（"other_player_only"）に限り、DO のシステム/部屋メッセージは含めない。他クレートは触らず、wasm 再ビルドなし、利用者に見える変化なし。`cargo test -p protocol` で完結する要石。

## 不変の原則

- **核は typed・純粋・注入で受ける**: nonce・board_hash・Action は呼び出し側が用意。核は sfen/usi 解釈も乱数も持たない（`TurnSession` の流儀を継ぐ）。
- **挙動保存**: 出典（protocol-wasm）の握り潰し／伝播・送信順・先着バッファ・turn 解放を一字一句なぞる。差異は再接続の照合の引き上げ**のみ**（決定 3）。
- **境界を核で綺麗にする**: `WireMessage` は対局チャネルの語彙に限る。DO のシステムメッセージは殻の関心事——核へ持ち込まない。
- **並存で衝突しない**: 既存 `NetMessage`・`ProtocolSession` はこの段で残す。置換は第二・三段で。
- **過ぎたるは及ばざる**: 統一ディスパッチャや action 型の汎化はしない。7 variant の enum と直截な feed で足りる。
