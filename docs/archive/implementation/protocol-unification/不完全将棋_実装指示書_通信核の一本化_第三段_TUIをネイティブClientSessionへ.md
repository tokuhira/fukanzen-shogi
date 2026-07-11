# 不完全将棋 実装指示書 — 通信核の一本化 第三段：TUI をネイティブ `ClientSession` へ（LAN 殻は TCP のまま・再接続再定義）

> 対象実行者: Claude Code（Sonnet 5）
> 前提: 第二段着地（HEAD `9a08595`。web が `ClientSession` を通り、PROTOCOL 5・`?v=`0.11.12）。TUI は**まだ旧実装**——`tui/src/net.rs` の `NetMessage`（手書き語彙）＋`perform_version_negotiation`（別メッセージの版交渉）、`tui/src/online.rs` の inline orchestration（`TurnSession` 直駆動＋`pending_peer_commit` ローカル＋`NetMessage` 手組み）＋`reconnect`（`RecoverySession.verify_identity` で生 secret 照合）。この段で TUI を**ネイティブ `protocol::ClientSession` 駆動**へ載せ替える。**LAN のトランスポート殻（TCP・4byte 長さプレフィックス）はそのまま**。再接続は決定 3 に従い**本人照合を核へ・再開点確認を殻に**割る。これは coupling が強い一続きの変更（`NetMessage`→`WireMessage` は全か無か）。`cargo build -p tui` が通り、二つの TUI を localhost で相互対戦（LAN 自己対戦）で検証する。この箱では cargo が走らないので、ビルド・LAN 検証は Sonnet 側。
> 関連する現物（すべて実地で確認済み・HEAD `9a08595` 基準）:
> - **核の API（第一段）** `protocol::ClientSession`: `new(Side, &[u8])` / `hello_msg()->WireMessage`(Hello) / `commit(BoardHash, Action, Nonce)->Result<WireMessage,SessionError>`(Commit) / `both_committed()->bool` / `reveal_msg()->Result<_,_>`(Reveal) / `ack_msg()->Result<_,_>`(Ack) / `feed(WireMessage)->Result<SessionEvent,SessionError>` / `reconnect_msg(BoardHash)->WireMessage`(Reconnect) / `reconnect_ack_msg(BoardHash)->WireMessage`(ReconnectAck) / `peer_auth_hash()->Option<SecretHash>` / `handshake_done()->bool`。`SessionEvent`（HandshakeDone{peer_side:Side} / PeerCommitted{both_committed} / PeerCommitBuffered / PeerRevealed{both_revealed} / PeerAcked / TurnComplete{sente:Action,gote:Action} / PeerReconnectRequest{board_hash:BoardHash} / ReconnectAck{resume_hash:BoardHash} / PeerAborted{reason:String}）。`SessionError`（VersionMismatch(NegotiationOutcome) / DuplicateHello / HandshakeNotDone / NoActiveTurn / Protocol(ProtocolError) / IdentityMismatch / BadHex / InvalidUsi）。`WireMessage` は serde（`#[serde(tag="type")]`）——net の serde 送受信にそのまま乗る。
> - **web の使い方が参照（第二段の現物）** `protocol-wasm/src/lib.rs`: `commit_move`＝parse→`inner.commit`→`both_committed()` で reveal 判断、`feed`→`SessionEvent` で分岐、再接続は core 照合＋`peer_reconnect_rejected`。TUI もこの駆動を native で写す。
> - **載せ替える殻** `tui/src/net.rs`: 既に「トランスポート共通（`NetMessage`/`NetEvent`/`Connection` の `.send()`/`.events`）／TCP 固有（`listen`/`connect`/`reader_loop`/`send` の `[TCP framing]`）」が分離済み。`Connection::{listen,connect,from_stream,send,perform_version_negotiation}`、`reader_loop`（4byte 長さ＋`serde_json::from_slice`）。hex ヘルパ群（`to_hex`/`from_hex`/`commitment_from_hex`/`nonce_from_hex`/`board_hash_from_hex`/`*_to_hex`）。`NegotiationError`。
> - **載せ替える消費側** `tui/src/online.rs`（721 行）: `run_online`（接続→`perform_version_negotiation`→`GameStart`+`wait_game_start`→App 準備→メインループ）、`handle_net_message`（`Commit`/`Reveal`/`Ack`/`Abort` を `TurnSession` 直駆動、`pending_peer_commit` 適用、投了判定＋`resolve`）、`wait_game_start`、`reconnect`（背景スレッド・`RecoverySession.verify_identity`（生 secret）＋`find_resume_point`）、`format_negotiation_error`、`OnlinePhase`、`sync_online_status`、`notify_reconnect`。`OnlineConfig{local_side, mode:ConnectMode, secret}`、`ConnectMode{Listen(u16),Connect(String)}`。
> - **再開点探索** `protocol::RecoverySession`: `new(Kifu, SecretHash)` / `find_resume_point(BoardHash)->Option<Position>`（kifu を初手から走査）/ `verify_identity(&[u8])->bool`（生 secret 照合＝**第三段では使わなくなる**）/ `current_hash()`。
> 関連文書: `不完全将棋_実装指示書_通信核の一本化アーク_概観と段組`（四層・段組・版の物語）、第一段・第二段指示書、`archive/implementation/不完全将棋_実装指示書_Phase3_TCP通信秘匿対戦`（TUI 秘匿対戦の由来）。
> 性格: 第三段は**「TUI をネイティブ `ClientSession` 駆動へ載せ替え、LAN の TCP 殻を保ちつつ、ワイヤ語彙を `WireMessage` に統一し、版交渉を hello に畳み、再接続を核照合モデルへ再定義する」**。対局の体験・LAN という能力は不変。挙動保存の急所は「先着 commit のバッファ」「投了→即終局」「切断→非ブロッキング再接続＋ロールバック通知」。**これは一続きの coupled 変更**——`NetMessage` を消す以上、handshake・turn loop・reconnect が同時に `WireMessage`/`ClientSession` へ移る。だが検証は段階的に（まず compile＋LAN 通常対戦、次に再接続）。PROTOCOL 5 を TUI が名乗るので旧 TUI（v0.11.2）とは LAN 非互換になる（版交渉が弾く）。配布版はパッチ bump を推奨（§5）。

---

## 0. 目的と範囲

- **作るもの（四部・一続きに compile する）**:
  1. **net.rs を `WireMessage` の TCP 殻へ**（§1）: `NetMessage`・`perform_version_negotiation`・`NegotiationError`・hex ヘルパ群を削除。`NetEvent::Message(WireMessage)`、`Connection::send(&WireMessage)`、`reader_loop` は `WireMessage` を deserialize。TCP framing はそのまま。
  2. **online.rs handshake を hello 交換へ**（§2）: `perform_version_negotiation`＋`GameStart`＋`wait_game_start` を、`ClientSession::new`→`hello_msg` 送信→peer Hello 受信→`feed(Hello)` に置換。版交渉は `feed(Hello)` の中（`SessionError::VersionMismatch`）。`peer_secret_hash` ローカルは消え、`session` が peer_auth_hash を保持。
  3. **online.rs turn loop を `ClientSession` 駆動へ**（§3）: `turn_session`・`pending_peer_commit` ローカルを廃し、永続 `session: ClientSession` に一本化。着手確定→`session.commit`、`PeerCommitted{both}`→`reveal_msg`、`PeerRevealed{both}`→`ack_msg`、`TurnComplete`→投了判定＋`resolve`。`handle_net_message` は `session.feed`→`SessionEvent` 分岐へ書き換え。
  4. **online.rs reconnect を再定義**（§4）: 生 secret を送らず `session.reconnect_msg(bh)`（auth_hash）。本人照合は `session.feed(Reconnect)`（`IdentityMismatch`）。再開点は殻の `RecoverySession.find_resume_point`。背景スレッドは**ソケット再確立のみ**担い、Reconnect 交換はメインループが永続 `session` で駆動（§4 の R1）。
- **位置づけ**: 通信核の一本化アークの**第三段**。LAN が新ワイヤ（PROTOCOL 5）で喋り、TUI と web が同じ核を共有する。第四段（WS 殻・クラウド参加）の土台。
- **作らないもの（＝理由つき）**:
  - **WS 殻・クラウド参加**: 第四段。この段は LAN（TCP）のみ。`OnlineConfig`/`ConnectMode` は Listen/Connect のまま。
  - **cli/cli-wasm の変更**: online 対戦は tui のみ。cli は無関係。
  - **`App`・`ui`・`input` の変更**: ゲームロジック・描画・入力は再利用（tui の北極星「ゲームロジックは App を再利用する」を保つ）。触るのは net.rs と online.rs のみ。
  - **`OnlinePhase` の意味変更**: WaitingMyMove/WaitingPeerCommit/WaitingPeerReveal/WaitingPeerAck/Disconnected/Aborted はそのまま（`SessionEvent` から再ソースするだけ）。
  - **統一ディスパッチャの汎化**: 直截な feed→match で足りる（過ぎたるは及ばざる）。

---

## 1. net.rs — `WireMessage` の TCP 殻（語彙を核へ・framing は殻に）

net.rs から**プロトコル意味を全て抜き**、TCP の送受信＋framing だけ残す。

- **削除**:
  - `enum NetMessage`（全 7 variant）。以後 `protocol::WireMessage` を使う。
  - `Connection::perform_version_negotiation`（版交渉は `ClientSession.feed(Hello)` へ移った）。
  - `enum NegotiationError`（net が版交渉をしなくなる）。
  - hex ヘルパ群（`to_hex`/`from_hex`/`commitment_to_hex`/`commitment_from_hex`/`nonce_to_hex`/`nonce_from_hex`/`board_hash_to_hex`/`board_hash_from_hex`）。hex 変換は `ClientSession`（`wire::to_hex`/`from_hex32`）が持つ。online.rs も手組みしなくなる。
- **変更**:
  - import: `use protocol::WireMessage;`（`BoardHash/Commitment/Nonce` は不要になる）。
  - `NetEvent::Message(WireMessage)`。
  - `Connection::send(&mut self, msg: &WireMessage) -> io::Result<()>`: 中身は現行のまま（`serde_json::to_vec(msg)`＋4byte 長さ＋write）。`WireMessage` は `Serialize` なので無改造で乗る。
  - `reader_loop`: `serde_json::from_slice::<WireMessage>(&body)`。framing・切断判定は現行のまま。
- **保つ**: `Connection{stream, events}`、`listen`/`connect`/`from_stream`、`[TCP framing]` の 4byte 長さプレフィックスと 1MiB 上限、`reader_loop` の切断→`Disconnected`。
- **doc コメント更新**: レイヤー図の `NetMessage` を `WireMessage` に、版交渉の記述を「版交渉は `ClientSession` が hello で行う」に直す。

**受け入れ（§1 単独）**: net.rs が `WireMessage` を送受信する TCP 殻になり、プロトコル語彙・版交渉・hex 変換を持たない。`cargo build -p tui` はまだ online.rs 側が未改修なら通らない——§1〜§4 は一続きに compile する。

## 2. online.rs — handshake を hello 交換へ

`run_online` 冒頭の「接続→版交渉→GameStart→wait_game_start」を、hello 交換へ置き換える。

- 接続（listen/connect）は現行のまま。
- 接続直後に `let mut session = ClientSession::new(config.local_side, &config.secret);`。
- `conn.send(&session.hello_msg())?;`（`WireMessage::Hello`）。
- peer の Hello を待って feed する補助関数を新設（`wait_game_start` を置換）:

```rust
/// peer の Hello を待って feed する。成功で peer_side、失敗で整形済みエラー文字列。
fn wait_and_feed_hello(
    conn: &mut Connection,
    session: &mut ClientSession,
) -> Result<Side, String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if std::time::Instant::now() > deadline {
            return Err("版交渉: 相手が応答しませんでした（版交渉未対応の版かもしれません）".to_string());
        }
        match conn.events.try_recv() {
            Ok(NetEvent::Message(wire @ WireMessage::Hello { .. })) => {
                return match session.feed(wire) {
                    Ok(SessionEvent::HandshakeDone { peer_side }) => Ok(peer_side),
                    Err(SessionError::VersionMismatch(o)) => Err(format_version_mismatch(&o)),
                    Err(e) => Err(format!("ハンドシェイク失敗: {:?}", e)),
                    Ok(_) => Err("ハンドシェイク: 予期しない応答".to_string()),
                };
            }
            Ok(NetEvent::Message(_)) => return Err("ハンドシェイク: hello 以外を受信".to_string()),
            Ok(NetEvent::Disconnected) => return Err("ハンドシェイク中に切断".to_string()),
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}
```

- `run_online` は `wait_and_feed_hello` の `Err` を `version_err` として現行同様に扱う（Aborted 表示）。`Ok(peer_side)` で通常進行。
- **`peer_secret_hash` ローカルは廃止**。再接続の本人照合に使っていた値は `session.peer_auth_hash()` が保持する。
- `format_negotiation_error`（`NegotiationError` を受ける）は削除し、`format_version_mismatch(&NegotiationOutcome)->String`（`SessionError::VersionMismatch` の中身を整形。現行の rule/protocol 不一致・Invalid・Timeout の文言を流用）へ書き換える。

## 3. online.rs — turn loop を `ClientSession` 駆動へ

`turn_session: Option<TurnSession>` と `pending_peer_commit: Option<Commitment>` の**ローカルを廃し**、§2 で作った永続 `session: ClientSession` に一本化する。

### 3.1 着手確定時（現行「着手が確定したか検出」ブロック）

```rust
if let Some(action) = my_action {
    app.phase = Phase::ResolveReady;
    let pos = kifu.current();
    let la = legal_actions(&pos, config.local_side);
    app.message = format!("着手確定: {}", ja_notation(&action, config.local_side, &la, &pos));

    let bh = board_hash(&pos);
    let nonce = random_nonce();
    match session.commit(bh, action, nonce) {
        Ok(commit_msg) => {
            conn.send(&commit_msg)?;                 // WireMessage::Commit
            online_phase = OnlinePhase::WaitingPeerCommit;
            sync_online_status(&mut app, &online_phase, config.local_side, true);
            // 先着していた peer commit があれば commit() の中で適用済み → 両者揃っていれば即 reveal
            if session.both_committed() {
                let reveal = session.reveal_msg().expect("both_committed 済みなら reveal 可");
                conn.send(&reveal)?;
                online_phase = OnlinePhase::WaitingPeerReveal;
                sync_online_status(&mut app, &online_phase, config.local_side, true);
            }
        }
        Err(e) => {
            let reason = format!("commit エラー: {:?}", e);
            online_phase = OnlinePhase::Aborted(reason.clone());
            let _ = conn.send(&WireMessage::Abort { reason });
        }
    }
}
```

- **先着バッファは `ClientSession` が持つ**（online.rs のローカル `pending_peer_commit` は不要）。§3.2 で feed した先着 Commit は `session` にバッファされ、`session.commit()` の中で適用される。だから着手確定後に `both_committed()` を見れば足りる。

### 3.2 ネットイベント処理（`handle_net_message` を `session.feed` 分岐へ）

`NetEvent::Message(wire)` を `session.feed(wire)` に渡し、`SessionEvent` で分岐する。現行 `handle_net_message` の 8 引数手続きを、この分岐へ置き換える。

```rust
NetEvent::Message(wire) => {
    match session.feed(wire) {
        Ok(SessionEvent::PeerCommitted { both_committed }) => {
            if both_committed {
                match session.reveal_msg() {
                    Ok(reveal) => {
                        conn.send(&reveal)?;
                        online_phase = OnlinePhase::WaitingPeerReveal;
                        app.message = "Reveal 送信済み — 相手の Reveal 待ち...".to_string();
                    }
                    Err(e) => abort(&mut online_phase, &mut conn, format!("reveal 生成エラー: {:?}", e)),
                }
            } else {
                app.message = "相手のコミット受信済み — 自分の着手を確定してください".to_string();
            }
        }
        Ok(SessionEvent::PeerCommitBuffered) => {
            app.message = "相手のコミット受信済み — 着手を入力してください".to_string();
        }
        Ok(SessionEvent::PeerRevealed { both_revealed }) => {
            if both_revealed {
                match session.ack_msg() {
                    Ok(ack) => {
                        conn.send(&ack)?;
                        online_phase = OnlinePhase::WaitingPeerAck;
                        app.message = "Ack 送信済み — 相手の Ack 待ち...".to_string();
                    }
                    Err(e) => abort(&mut online_phase, &mut conn, format!("ack エラー: {:?}", e)),
                }
            }
        }
        Ok(SessionEvent::TurnComplete { sente, gote }) => {
            resolve_completed_turn(sente, gote, &mut app, &mut kifu, &mut online_phase, config.local_side);
        }
        Ok(SessionEvent::PeerAborted { reason }) => {
            abort_to(&mut online_phase, format!("相手がアボート: {}", reason));
        }
        // 再接続（§4）
        Ok(SessionEvent::PeerReconnectRequest { board_hash }) => { /* §4.2 */ }
        Ok(SessionEvent::ReconnectAck { resume_hash }) => { /* §4.2 */ }
        Err(SessionError::IdentityMismatch) => {
            let _ = conn.send(&WireMessage::Abort { reason: "auth_mismatch".to_string() });
            abort_to(&mut online_phase, "再接続: 認証失敗".to_string());
        }
        Err(e) => {
            let reason = format!("プロトコルエラー: {:?}", e);
            let _ = conn.send(&WireMessage::Abort { reason: reason.clone() });
            abort_to(&mut online_phase, reason);
        }
        Ok(SessionEvent::HandshakeDone { .. }) | Ok(SessionEvent::PeerAcked) => { /* ループ中は無視/待機 */ }
    }
    sync_online_status(&mut app, &online_phase, config.local_side, true);
}
```

- `resolve_completed_turn` は現行 `handle_net_message` の `Ack`→`is_complete` 分岐の中身をそのまま関数化する（**投了判定を保存**）: `sente_action.is_resign()/gote_action.is_resign()` で `GameOverKind::{Draw(MutualResign)/GoteWins(Resign)/SenteWins(Resign)}`、通常手は `app.sente_action/gote_action` セット→`app.resolve_turn()`→`kifu.push(Ply{sente,gote})`→次ターン（後手は `Phase::GoteInput`＋`cursor_rank=1`）。挙動を一字一句保つ。
- `abort`/`abort_to` は小さなヘルパ（`online_phase = Aborted(reason)` ＋必要なら `conn.send(Abort)`）。現行の `Err(abort_reason)` 返し＋`conn.send(Abort)` と同じ挙動に。

### 3.3 投了キー（`r`）

現行のまま（`app.sente_action/gote_action = Some(Action::Resign)`）。§3.1 が Resign を `session.commit` に通し、§3.2 の `TurnComplete` で終局する。`Action::Resign` は `ClientSession`/`TurnSession` テストに前例あり（投了 commit の拘束性も核が守る）。

## 4. online.rs — 再接続の再定義（最も繊細な部・R1）

**割り方（決定 3）**: 本人照合は `ClientSession`（`feed(Reconnect)` が auth_hash を照合）。再開点確認は殻（`RecoverySession.find_resume_point`）。生 secret はワイヤに出さない。

**スレッド構造（R1）**: 背景スレッドは**ソケット再確立のみ**（listen/connect のリトライ）を担い、再確立した `Connection` をメインへ返す。**Reconnect 交換はメインループが永続 `session` で駆動**する（`session` はメインスレッドに在るので照合ロジックをそこへ置く。split-brain を避ける）。`session` は切断でリセットしない（peer_auth_hash・handshake_done を保持）。

### 4.1 切断検出（現行 `NetEvent::Disconnected` 分岐の改修）

- 現行どおり `move_rolled_back` を記録（切断時に自分の着手が確定済みだったか）。
- `session` は**保持**（`turn_session=None; pending_peer_commit=None` の行は廃止——バッファは session が持つ。ただし進行中ターンの扱いは §4.4 参照）。
- 背景スレッドは `reconnect_socket_only(&config)`（下記）を呼び、`ReconnectEvent::Success(Connection)` / `Failed(String)` を返す。`peer_secret_hash` の受け渡しは**不要**（照合は session がやる）。

```rust
/// ソケット再確立のみ（Reconnect 交換はしない）。現行 reconnect() の前半だけ。
fn reconnect_socket_only(config: &OnlineConfig) -> io::Result<Connection> {
    match &config.mode {
        ConnectMode::Listen(port) => Connection::listen(*port),
        ConnectMode::Connect(addr) => {
            let deadline = std::time::Instant::now() + Duration::from_secs(60);
            loop {
                match Connection::connect(addr) {
                    Ok(c) => return Ok(c),
                    Err(_) if std::time::Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(500));
                    }
                    Err(e) => return Err(e),
                }
            }
        }
    }
}
```

### 4.2 再確立後（メインループの `ReconnectEvent::Success` 受け取り）

```rust
ReconnectEvent::Success(new_conn) => {
    conn = new_conn;
    // 自分の現局面ハッシュで reconnect を送る（auth_hash は session が入れる）。
    let bh = board_hash(&kifu.current());
    let _ = conn.send(&session.reconnect_msg(bh));
    online_phase = OnlinePhase::WaitingMyMove; // 再開待ち（現行同様、必要なら専用フェーズ）
    // ロールバック通知は現行の move_rolled_back 分岐を踏襲
    ...
}
```

- **相手の Reconnect / ReconnectAck は §3.2 の feed 分岐で処理する**（別経路を作らない）:
  - `PeerReconnectRequest { board_hash }`（core が本人照合済み）: `RecoverySession::new(kifu.clone(), session.peer_auth_hash().unwrap_or_default_hash()).find_resume_point(board_hash)` で再開点を探す。見つかれば `conn.send(&session.reconnect_ack_msg(board_hash))` ＋ その Position へ resume（現行 `onResumeAt` 相当の App 反映）。見つからなければ `conn.send(&WireMessage::Abort{reason:"hash_mismatch"})` ＋ Aborted。
  - `ReconnectAck { resume_hash }`: `find_resume_point(resume_hash)` で再開点を確定し resume。見つからなければ Aborted。
  - `IdentityMismatch`（§3.2 で処理済み）: 相手へ Abort＋「再接続: 認証失敗」。

- **`RecoverySession` の使い方が変わる**: `verify_identity`（生 secret 照合）は**使わない**（core が auth_hash で照合）。`find_resume_point` のみ使う。`RecoverySession::new` は `SecretHash` を要るが、再開点探索はハッシュを使わない（kifu 走査のみ）ので、`session.peer_auth_hash()` を渡せば足りる（`None` の理論上ケースはゼロ値でよい——handshake 済みなら必ず `Some`）。※`find_resume_point` だけを使うなら、`RecoverySession` を介さず kifu を直接走査する小関数にしてもよい（`RecoverySession` の `verify_identity`/secret_hash が不要になるため）。実装者判断でどちらでも可——ただし挙動（初手から走査し一致 Position を返す）は保つ。

### 4.3 P2P の対称性（確認）

TCP 切断は両側が検出する。両側が `reconnect_socket_only`（一方 listen・一方 connect）→ 両側が `reconnect_msg` 送信 → 両側が相手の Reconnect を feed → 両側が `PeerReconnectRequest` で再開点確認 → 両側が `reconnect_ack` → 両側が `ReconnectAck` で resume。共有 secret ゆえ両 auth_hash は一致し、各々が相手の auth_hash を hello 時の peer_auth_hash と照合して通る。対称に成立する。

### 4.4 進行中ターンの扱い（挙動保存）

現行は切断時に `turn_session=None` でターンを捨て、再接続後 `WaitingMyMove` で着手し直し（`move_rolled_back` 通知）。第三段でも**同じ挙動を保つ**: 切断時に `session` の進行中ターンを捨てる必要がある。`ClientSession` に「進行中ターンを破棄する」手段が無ければ、**再接続成功時に `session` を作り直す**のではなく（peer_auth_hash を失う）、切断時点で `session` の turn だけリセットしたい。最小手段として、`ClientSession` に `abort_turn(&mut self)`（`self.turn=None; self.pending_peer_commit=None;` handshake_done と peer_auth_hash は保持）を**第一段の核へ小さく追加**してよい（純粋・テスト可能）。これは第一段の範囲外の小追加なので、この段で `protocol` に足す場合は `cargo test -p protocol` に 1 テスト（abort 後も handshake_done/peer_auth_hash が残る）を添えること。※もし turn を捨てず「途中から再開」を目指すなら設計が重くなる——現行挙動（捨てて指し直し）を保つのが過ぎたるは及ばざる。

## 5. ビルド・テスト・受け入れ・版

- **段階的検証**:
  1. `cargo build -p tui` が通る（§1〜§4 一続き）。`cargo test -p protocol`（§4.4 で `abort_turn` を足したならそのテスト込み）緑。`cargo clippy` 警告なし。
  2. **LAN 通常対戦**: 二つの TUI を localhost で（一方 `Listen(port)`・一方 `Connect(addr)`）。hello 交換→複数手→通常終局／投了。先手・後手両視点で着手ペアが正しく、投了が即終局すること。**先着 commit**（相手が先に指す）でもバッファ→両者揃いで reveal が流れること。
  3. **版不一致**: 片方を PROTOCOL 4（旧）に見立てた hello（または proto_ver 改変）で、版交渉が弾き Aborted 表示になること。
  4. **再接続**: 対局中に一方の TCP を落とし（プロセス片方を落とす/ネットワーク遮断）、再接続でソケット再確立→reconnect 交換→再開点一致で resume。着手確定後に落ちた場合の `move_rolled_back` 通知。別 secret での再接続が `IdentityMismatch`→Abort になること。
- **受け入れ条件**:
  - net.rs が `WireMessage` の TCP 殻（`NetMessage`/版交渉/hex ヘルパが消えている）。
  - online.rs が永続 `ClientSession` 駆動（`turn_session`/`pending_peer_commit` ローカルが消え、`handle_net_message` が `feed`→`SessionEvent` 分岐に）。
  - 再接続が core 照合（auth_hash）＋殻の再開点探索で成立し、生 secret がワイヤに出ない。
  - 投了・先着バッファ・ロールバック通知の挙動が保存。
  - `protocol`（core）は §4.4 の `abort_turn` 追加を除き無変更。web・server は無変更。
- **版**: TUI バイナリのワイヤが PROTOCOL 5 になる（旧 TUI と LAN 非互換）。利用者に見える新能力ではない（LAN のまま）が、**配布バイナリの挙動が変わる**ので配布版は**パッチ bump 推奨**（v0.11.2 → v0.11.3）。クラウド参加という新能力が立つ第四段でマイナー bump。最終判断は作り手。

## 末尾要約

TUI を `protocol::ClientSession` のネイティブ駆動へ載せ替える。net.rs は `WireMessage` を送受信する TCP 殻へ痩せ（`NetMessage`・版交渉・hex ヘルパを削除、framing は保つ）、online.rs は handshake を hello 交換へ（版交渉は `feed(Hello)` の中）、turn loop を永続 `session` の `commit`/`feed`/`reveal_msg`/`ack_msg` 駆動へ、再接続を核照合（auth_hash）＋殻の再開点探索（`find_resume_point`）へ再定義する。背景スレッドはソケット再確立のみ、Reconnect 交換はメインループが永続 session で駆動（R1）。先着バッファ・投了即終局・ロールバック通知の挙動を保存。LAN という能力は不変、ワイヤは PROTOCOL 5。二つの TUI の LAN 自己対戦（通常＋再接続）で検証。配布版パッチ bump 推奨。

## 不変の原則

- **核へ寄せ、殻に薄いラッパ**: 語彙（`WireMessage`）と orchestration（`ClientSession`）は核。net.rs は TCP framing だけの殻。
- **App を再利用する**（tui の北極星）: ゲームロジック・描画・入力は不変。触るのは net.rs と online.rs のみ。
- **照合は核・再開点は殻**（決定 3）: 再接続の本人照合は `ClientSession.feed`、再開点探索は kifu を持つ殻。生 secret はワイヤに出さない。
- **挙動保存の急所**: 先着 commit のバッファ（今は session が持つ）、投了→即終局、切断→非ブロッキング再接続＋進行中ターン破棄＋ロールバック通知。
- **一続きに compile・段階的に検証**: `NetMessage`→`WireMessage` は全か無か。だが検証は compile→LAN 通常→版不一致→再接続の順に刻む。
- **過ぎたるは及ばざる**: 途中再開の凝った復元はしない（捨てて指し直しを保つ）。`RecoverySession` は `find_resume_point` だけ使い、`verify_identity` は退役。
