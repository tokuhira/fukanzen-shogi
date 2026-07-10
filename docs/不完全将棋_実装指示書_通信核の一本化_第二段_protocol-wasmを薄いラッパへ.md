# 不完全将棋 実装指示書 — 通信核の一本化 第二段：protocol-wasm を薄いラッパへ・再接続を核照合へ（2a）

> 対象実行者: Claude Code（Sonnet 5）
> 前提: 第一段着地（HEAD `129f274`。fmt 追いコミット込み）。`protocol` に `WireMessage`（serde 正本）・`ClientSession`（transport 非依存 orchestration）・`PROTOCOL_VERSION = 5` が据わっている。`protocol-wasm` の `ProtocolSession` と `web/online.js` は**旧実装のまま**。この段で protocol-wasm を核の薄いラッパへ痩せさせ、online.js の再接続だけを核照合モデルへ寄せる。**対局フロー（hello/commit/reveal/ack）のワイヤ・バイト列と JS 向け契約は保存**する。wasm 再ビルド要（この箱では走らないので Sonnet 側）。
> 関連する現物（すべて実地で確認済み・HEAD `129f274` 基準）:
> - **核の API（第一段の現物）** `protocol::client::ClientSession`: `new(side: Side, secret: &[u8])` / `hello_msg()->WireMessage`(Hello) / `reconnect_msg(BoardHash)->WireMessage`(Reconnect) / `reconnect_ack_msg(BoardHash)->WireMessage`(ReconnectAck) / `commit(BoardHash, Action, Nonce)->Result<WireMessage,SessionError>`(Commit) / `both_committed()->bool` / `reveal_msg()->Result<WireMessage,SessionError>`(Reveal) / `ack_msg()->Result<WireMessage,SessionError>`(Ack) / `feed(WireMessage)->Result<SessionEvent,SessionError>` / `peer_auth_hash()->Option<SecretHash>` / `handshake_done()->bool`。`SessionEvent`（HandshakeDone{peer_side:Side} / PeerCommitted{both_committed} / PeerCommitBuffered / PeerRevealed{both_revealed} / PeerAcked / TurnComplete{sente:Action,gote:Action} / PeerReconnectRequest{board_hash:BoardHash} / ReconnectAck{resume_hash:BoardHash} / PeerAborted{reason:String}）。`SessionError`（VersionMismatch(NegotiationOutcome) / DuplicateHello / HandshakeNotDone / NoActiveTurn / Protocol(ProtocolError) / IdentityMismatch / BadHex / InvalidUsi）。`WireMessage::{to_json()->String, from_json(&str)->Result<_,WireError>}`、`WireError::{InvalidJson, UnknownType}`、`wire::{to_hex(&[u8])->String, from_hex32(&str)->Option<[u8;32]>}`。
> - **痩せさせる対象** `protocol-wasm/src/lib.rs` 現行 `ProtocolSession`。その **JS 向け契約**（online.js が依存する形）:
>   - `new ProtocolSession(side:&str, secret:&str)`
>   - `hello_msg()->String`＝**裸の** JSON（`{"type":"hello",...}`。online.js は `_wsSend(session.hello_msg())` でそのまま送る・parse しない）
>   - `commit_move(sfen, usi)->String`＝`{"ok":true,"message":{...commit...},"both_committed":<bool>}` / `{"ok":false,"error":"..."}`
>   - `reveal_msg()->String` / `ack_msg()->String`＝`{"ok":true,"message":{...}}` / `{"ok":false,"error":"..."}`
>   - `reconnect_msg(hash)->String`＝**`{ok,message}` で包む**（`{"ok":true,"message":{"type":"reconnect",...}}`。hello_msg が裸なのと非対称だが online.js はこの形で消費する）
>   - `peer_auth_hash()->String`＝hex（2a 後は online.js から呼ばれなくなる）
>   - `feed(str)->String`＝`{"ok":true,"event":"...",...}` / `{"ok":false,"error":"..."}`。旧イベント: handshake_done{peer_side} / peer_committed{both_committed} / peer_commit_buffered / peer_revealed{both_revealed} / peer_acked / turn_complete{sente_usi,gote_usi} / peer_aborted{reason} / **peer_reconnect_request{auth_hash,board_hash}** / reconnect_ack{resume_hash}
>   - free fn `sfen_hash(sfen)->String`（hex or 空）・`version_tuple()->String`。**両者そのまま残す**（ClientSession を経由しない。`sfen_hash` は `board_hash` 直呼び、`version_tuple` は `MY_VERSION` を読む＝自動で protocol 5 を名乗る）。
> - **online.js の消費点**（HEAD 基準の行）: `commitMoveOnline` 143–152（ok/message/both_committed を読む）・hello 送信 223（`_wsSend(session.hello_msg())`）・feed dispatch 322–（ok・event・各フィールド）・再接続 `peer_reconnect_request` 377–395（**`session.peer_auth_hash()` を読んで JS 側で照合し、不一致なら相手へ `abort{auth_mismatch}` を送る**）・`reconnect_ack` 397–。DO システムメッセージは 219–317 で**すべて return フィルタ**され feed には来ない。
> 関連文書: `不完全将棋_実装指示書_通信核の一本化アーク_概観と段組`（四層・段組・版の物語）、`不完全将棋_実装指示書_通信核の一本化_第一段_protocol核へWireMessageとClientSession`。
> 性格: 第二段は**「protocol-wasm の `ProtocolSession` を `protocol::ClientSession` の薄い wasm_bindgen ラッパへ痩せさせ、online.js の再接続を核照合モデル（2a）へ寄せる」**。**対局フローは JS 向け契約もワイヤ・バイト列も保存**（web の挙動不変）。**再接続経路のみ意図的に作り替える**——本人照合は核が済ませ、online.js は再開点確認だけを担う（決定 3）。DO（server/）は無変更（ワイヤ不変）。web が実 DO で緑なら、核が実地で正しい証明になる。web `?v=` を上げる（キャッシュ）・配布版は据え置き。

---

## 0. 目的と範囲

- **作るもの**:
  1. `protocol-wasm/src/lib.rs` — `ProtocolSession` を `ClientSession` の薄いラッパへ書き換え。JS 向け契約（§1）を保存し、再接続イベントを 2a へ（§1.7）。
  2. `web/online.js` — 再接続経路を核照合へ（§2）。JS 側の auth 照合を落とし、`peer_reconnect_rejected` イベントを受ける。
  3. web `?v=` の前進（protocol-wasm 再ビルドのキャッシュ更新）。
- **位置づけ**: 通信核の一本化アークの**第二段**。web が共有核（`ClientSession`）を通るようになる。web の緑＝核の実地検証。
- **作らないもの（＝理由つき）**:
  - **TUI（net.rs / online.rs）の変更**: 第三段。この段は web 側のみ。
  - **TUI の WS 殻・クラウド参加**: 第四段。
  - **server/（DO）の変更**: ワイヤ・バイト列が対局フローで不変なので DO は触らない。再接続の DO 枠組み（you_reconnected/peer_reconnected/request_reset）も不変——online.js のフィルタ（219–317）はそのまま。
  - **対局フローのワイヤ形・イベント形の変更**: hello/commit/reveal/ack と handshake_done/peer_committed/peer_revealed/turn_complete/peer_acked/peer_aborted は**バイト・フィールド名を保存**。
  - **`sfen_hash` / `version_tuple` の意味変更**: そのまま残す。
  - **`getrandom` の nonce 生成方式の変更**: 現行 `random_nonce`（getrandom）をラッパ側に残す（核は nonce を注入で受けるので、生成は殻＝ラッパの責務）。

---

## 1. `protocol-wasm/src/lib.rs`（薄いラッパ）

`ProtocolSession` は `inner: ClientSession` を一つ持つだけにする。各メソッドは inner を呼び、**旧 JS 契約の JSON 文字列**へ整形して返す。sfen/usi の解釈と nonce 生成は**ラッパが担う**（核は typed・純粋）。

### 1.1 import と補助

```rust
use wasm_bindgen::prelude::*;

use engine::serialize::sfen_to_position;
use engine::types::{Action, Side};
use protocol::client::{ClientSession, SessionError, SessionEvent};
use protocol::commit::Nonce;
use protocol::hash::{board_hash, BoardHash};       // board_hash は protocol::hash 経由（現行 import に合わせる）
use protocol::negotiate::MY_VERSION;
use protocol::wire::{from_hex32, to_hex, WireError, WireMessage};

fn random_nonce() -> Nonce {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).expect("getrandom failed");
    Nonce(bytes)
}

fn side_str(s: Side) -> &'static str {
    match s { Side::Sente => "sente", Side::Gote => "gote" }
}
fn err_json(code: &str) -> String {
    format!(r#"{{"ok":false,"error":"{}"}}"#, code)
}
fn wrapped_msg(m: WireMessage) -> String {
    format!(r#"{{"ok":true,"message":{}}}"#, m.to_json())
}
```

- `board_hash` の所在は現行 protocol-wasm の import（`protocol::hash::board_hash` か再エクスポート `protocol::board_hash`）に合わせる。どちらでも可。

### 1.2 free fn は現行のまま

`sfen_hash(sfen)` と `version_tuple()` は**現行実装をそのまま残す**（`ClientSession` を経由しない）。差分ゼロ。

### 1.3 構築・ハンドシェイク

```rust
#[wasm_bindgen]
pub struct ProtocolSession {
    inner: ClientSession,
}

#[wasm_bindgen]
impl ProtocolSession {
    #[wasm_bindgen(constructor)]
    pub fn new(side: &str, secret: &str) -> ProtocolSession {
        let s = if side == "sente" { Side::Sente } else { Side::Gote };
        ProtocolSession { inner: ClientSession::new(s, secret.as_bytes()) }
    }

    /// 裸の hello JSON（online.js は parse せず _wsSend でそのまま送る）。
    pub fn hello_msg(&self) -> String {
        self.inner.hello_msg().to_json()
    }

    /// {ok,message} で包む（旧契約どおり非対称）。
    pub fn reconnect_msg(&self, board_hash_hex: &str) -> String {
        match from_hex32(board_hash_hex) {
            Some(b) => wrapped_msg(self.inner.reconnect_msg(BoardHash(b))),
            None => err_json("invalid_board_hash"),
        }
    }

    /// 2a 後は online.js から呼ばれないが、契約保存のため残す（薄い鏡）。
    pub fn peer_auth_hash(&self) -> String {
        self.inner.peer_auth_hash().map(|h| to_hex(&h.0)).unwrap_or_default()
    }
```

### 1.4 commit_move（sfen/usi 解釈と nonce 生成はラッパ）

```rust
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
```

- usi/sfen の解釈エラーは**ラッパで先に**返す（旧 `commit_move` と同じ `invalid_usi`/`invalid_sfen`）。だから `inner.commit` の `SessionError::InvalidUsi` はこの経路では起きない（InvalidUsi は feed(Reveal) 側でのみ現れる）。

### 1.5 reveal_msg / ack_msg

```rust
    pub fn reveal_msg(&mut self) -> String {
        match self.inner.reveal_msg() {
            Ok(m) => wrapped_msg(m),
            Err(e) => err_json(&session_error_code(&e)),
        }
    }

    pub fn ack_msg(&mut self) -> String {
        match self.inner.ack_msg() {
            Ok(m) => wrapped_msg(m),   // Ack.to_json() == {"type":"ack"}
            Err(e) => err_json(&session_error_code(&e)),
        }
    }
```

### 1.6 feed（イベント整形・版不一致・2a 再接続拒否の特別扱い）

```rust
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
            Err(SessionError::VersionMismatch(o)) => format!(
                r#"{{"ok":false,"error":"version_mismatch","detail":"{:?}"}}"#, o
            ),
            // IdentityMismatch は feed(Reconnect) からのみ到達（2a）。
            // ok:false にすると online.js の汎用エラー経路で ws.close されてしまい、
            // 相手へ abort を送る礼儀が失われる。よって「ok:true の拒否イベント」で返し、
            // online.js が abort を送れるようにする（§2）。
            Err(SessionError::IdentityMismatch) =>
                r#"{"ok":true,"event":"peer_reconnect_rejected","reason":"auth_mismatch"}"#.to_string(),
            Err(e) => err_json(&session_error_code(&e)),
        }
    }
}
```

### 1.7 イベント・エラーの整形（ラッパ末尾の自由関数）

```rust
fn event_json(ev: SessionEvent) -> String {
    match ev {
        SessionEvent::HandshakeDone { peer_side } => format!(
            r#"{{"ok":true,"event":"handshake_done","peer_side":"{}"}}"#, side_str(peer_side)),
        SessionEvent::PeerCommitted { both_committed } => format!(
            r#"{{"ok":true,"event":"peer_committed","both_committed":{}}}"#, both_committed),
        SessionEvent::PeerCommitBuffered =>
            r#"{"ok":true,"event":"peer_commit_buffered"}"#.to_string(),
        SessionEvent::PeerRevealed { both_revealed } => format!(
            r#"{{"ok":true,"event":"peer_revealed","both_revealed":{}}}"#, both_revealed),
        SessionEvent::PeerAcked =>
            r#"{"ok":true,"event":"peer_acked"}"#.to_string(),
        SessionEvent::TurnComplete { sente, gote } => format!(
            r#"{{"ok":true,"event":"turn_complete","sente_usi":"{}","gote_usi":"{}"}}"#,
            sente.to_usi(), gote.to_usi()),
        // 2a: auth_hash は載せない（照合は核が済ませた）。board_hash のみ。
        SessionEvent::PeerReconnectRequest { board_hash } => format!(
            r#"{{"ok":true,"event":"peer_reconnect_request","board_hash":"{}"}}"#,
            to_hex(&board_hash.0)),
        SessionEvent::ReconnectAck { resume_hash } => format!(
            r#"{{"ok":true,"event":"reconnect_ack","resume_hash":"{}"}}"#,
            to_hex(&resume_hash.0)),
        SessionEvent::PeerAborted { reason } => format!(
            r#"{{"ok":true,"event":"peer_aborted","reason":"{}"}}"#, reason),
    }
}

fn session_error_code(e: &SessionError) -> String {
    match e {
        SessionError::DuplicateHello => "duplicate_hello".to_string(),
        SessionError::HandshakeNotDone => "handshake_not_done".to_string(),
        SessionError::NoActiveTurn => "no_active_turn".to_string(),
        SessionError::Protocol(pe) => format!("{:?}", pe),   // 旧 feed も {:?} で返していた
        SessionError::BadHex => "invalid_hex".to_string(),   // ★下記の綻び参照
        SessionError::InvalidUsi => "invalid_action".to_string(), // feed(Reveal) 文脈
        // VersionMismatch / IdentityMismatch は feed() で特別扱い（ここには来ない）
        SessionError::VersionMismatch(_) | SessionError::IdentityMismatch =>
            "unexpected".to_string(),
    }
}
```

**正直な綻び（一点・整形上の劣化・ロジック非影響）**: 旧 protocol-wasm は不正 hex を欄ごとに `invalid_commitment` / `invalid_nonce` / `invalid_board_hash` / `invalid_auth_hash` と区別して返していた。新 `SessionError::BadHex` は一括なので、ラッパは `invalid_hex` の一語に丸める。online.js はこの文字列を**表示するだけ**でロジック分岐しない（`_cbs?.onStatus('error', ...)`）ので実害はないが、エラー表示の粒度が落ちる。過ぎたるは及ばざるで欄別復元はしない。気になれば将来 `SessionError` を欄別 variant に割る余地はある（今は作らない）。

## 2. `web/online.js`（再接続を核照合へ・2a）

**変更は再接続の 2 箇所のみ。対局フローの dispatch（handshake_done〜peer_aborted）は一切触らない。**

### 2.1 `peer_reconnect_request` ケース（JS 照合を落とす）

現行（377–395 付近）の**先頭の auth 照合ブロックを削除**する。核が照合済みなので、残留側は再開点確認だけを行う。

```js
    case 'peer_reconnect_request': {
      // 本人照合は核（ClientSession）が済ませている。ここは再開点の確認のみ。
      const resumeSfen = _findSfenByHash(result.board_hash);
      if (!resumeSfen) {
        _wsSend(JSON.stringify({ type: 'abort', reason: 'hash_mismatch' }));
        _cbs?.onStatus('error', '再接続: 棋譜が一致しません');
        return;
      }
      _wsSend(JSON.stringify({ type: 'reconnect_ack', board_hash: result.board_hash }));
      _cbs?.onStatus('ready', '対局中');
      _cbs?.onResumeAt?.(resumeSfen);
      break;
    }
```

- 消える行: `const expectedAuthHash = session.peer_auth_hash();` と、それに続く `if (!expectedAuthHash || result.auth_hash !== expectedAuthHash) { …abort… return; }` の塊。
- `result.auth_hash` はもうイベントに載らない（§1.7）。参照も消える。

### 2.2 `peer_reconnect_rejected` ケース（新設・auth 不一致の礼儀を保つ）

核が本人照合に失敗したときにラッパが返す拒否イベント（§1.6）を受け、**旧来どおり相手へ abort を送り**エラー表示する。`switch (result.event)` に追加：

```js
    case 'peer_reconnect_rejected': {
      // 核が本人照合に失敗（auth_mismatch）。相手へ abort を送る礼儀を保つ。
      _wsSend(JSON.stringify({ type: 'abort', reason: result.reason }));
      _cbs?.onStatus('error', '再接続: 認証失敗');
      break;
    }
```

これで旧挙動（auth 不一致→相手へ `abort{auth_mismatch}`＋エラー表示）が保たれる。唯一の違いは、照合の主体が JS から核へ移ったこと（決定 3）。

### 2.3 それ以外

`session.peer_auth_hash()` の呼び出しは 2.1 の削除で消える（online.js からは以後未使用）。他は無変更。

## 3. ビルド・テスト・受け入れ

- **ビルド**: protocol-wasm を wasm-pack で再ビルドし、成果物を `web/protocol-wasm/` へ配置（現行の配置・手順に従う）。`cargo test -p protocol` は第一段のまま緑（この段は core を触らない）。
- **web `?v=`**: protocol-wasm を差し替えたので、`web/index.html` 等の `?v=` を前進（キャッシュ更新）。**配布版は据え置き**（利用者に見える新能力なし）。
- **web テスト**: 既存の web テスト（純粋モジュール群）が緑のまま（この段は純粋モジュールを触らないので無傷のはず）。
- **実 DO での手検証（この段の本丸）**:
  1. 二つのブラウザで一局を通す（部屋キーで入室→握手→複数手→投了か通常終局）。**DO を流れるワイヤ・バイト列が対局フローで従来と同一**であること（hello/commit/reveal/ack の JSON が変わっていない＝第一段の `byte_layout_matches_protocol_wasm` が保証する形）。
  2. **対局中に一方が切断→再接続**し、核照合経由で再開できること（`peer_reconnect_request`→再開点確認→`reconnect_ack`→resume）。
  3. **auth 不一致の再接続**（別 secret で無理やり reconnect）で、残留側が相手へ abort を送りエラー表示すること（`peer_reconnect_rejected` 経路）。
- **受け入れ条件**:
  - 対局フローの JS 契約・ワイヤ・バイト列が保存（web の通常挙動不変）。
  - 再接続が核照合で成立し、auth 不一致で abort＋エラー（礼儀保存）。
  - `protocol-wasm` が `ClientSession` の薄いラッパになっている（`TurnSession` 直駆動や JSON 手組みの二重管理が消えている）。
  - `server/`・`tui/`・`protocol`（core）に差分なし。

## 末尾要約

`protocol-wasm` の `ProtocolSession` を `protocol::ClientSession` の薄い wasm_bindgen ラッパへ痩せさせる。sfen/usi の解釈と nonce 生成はラッパが担い、対局フロー（hello/commit/reveal/ack）の JS 向け契約とワイヤ・バイト列を保存する。再接続だけは決定 3（2a）に従い作り替え——本人照合を核が済ませ、online.js は再開点確認のみ行い、核が返す `peer_reconnect_rejected`（auth 不一致）を受けて相手へ abort を送る礼儀を保つ。DO は無変更（ワイヤ不変）。web が実 DO で一局と再接続を緑で通れば、第一段の核が実地で正しい証明になる。web `?v=` 前進・配布版据え置き。

## 不変の原則

- **対局フローは保存**: hello/commit/reveal/ack の JS 契約・ワイヤ・バイト列・イベント形を一字一句保つ（web 挙動不変）。触るのは再接続の 2 箇所のみ。
- **殻が解釈・生成、核は typed・純粋**: sfen→board_hash・usi→Action・nonce 生成はラッパ（殻）の責務。`ClientSession` は typed な値で受ける。
- **照合は核へ（決定 3）**: 再接続の本人照合は `ClientSession.feed` が所有。online.js は再開点確認と礼儀（abort 送出）だけを残す。
- **DO は素通しのまま**: server/ を触らない。DO は commit-reveal を裁定せず relay する（審判なし）。
- **過ぎたるは及ばざる**: エラー文字列の欄別粒度は復元しない（`invalid_hex` 一語）。`peer_auth_hash()` は未使用になるが撤去は急がず残す。
