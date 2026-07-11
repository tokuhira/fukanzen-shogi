# 不完全将棋 実装指示書 — 通信核の一本化 第四段：TUI に WS 殻を足す＝クラウド参加（TUI ↔ ブラウザの相席）

> 対象実行者: Claude Code（Sonnet 5）
> 前提: 第三段着地（HEAD `2305baf`。TUI は永続 `ClientSession` 駆動、LAN は `WireMessage` の TCP 殻、PROTOCOL 5、配布 v0.11.3）。この段で TUI に **WebSocket の殻**を足し、web と同じ Cloudflare Durable Object（DO）の部屋へ入って対局する。**同じ `ClientSession` を、TCP でなく WS の上で駆動する**——核と交換可能な殻の結実。LAN（TCP）はそのまま併存。**side は listen/connect の選択でなく DO が告げる**（topology の転換）。再接続は DO の `you_reconnected`/`peer_reconnected` の枠組みに乗り、session レベルの再接続機構（第三段の `reconnect_msg`/`PeerReconnectRequest`/`ReconnectAck`/`find_resume_point`）を再利用する。ビルド・実 DO 検証は Sonnet 側（この箱は cargo も外部 WS も不可）。締めで配布 v0.12.0（クラウド参加＝新能力＝マイナー）。
> 関連する現物（すべて実地で確認済み・HEAD `2305baf` 基準）:
> - **手本＝web の WS クライアント** `web/online.js`: `WS_BASE_URL = 'wss://fukanzen-shogi-ws.tokuhira.workers.dev'`、接続は `new WebSocket(`${WS_BASE_URL}/room/${encodeURIComponent(roomKey)}`)`。流れ: 接続 → `peer_joined`/`room_ready`（`msg.your_side` で陣営決定）→ session 構築 → `hello_msg()` 送信 → `handshake_done`。再接続: `you_reconnected` 受信 → `reconnect` 送信（＋`request_reset` を送る枝, 行 290）→ `reconnect_ack` で再開。`peer_reconnected` 受信で残留側が応答。**DO システムメッセージ**（`peer_joined`/`room_ready`/`room_full`/`peer_disconnected`/`peer_reconnected`/`you_reconnected`）は online.js が game channel の前に捌く。spectate_*・record_* は**先手のみ**が送る（この段では作らない・§0 非ゴール）。
> - **核（第一〜三段）** `protocol::ClientSession`: `new(Side,&[u8])`/`hello_msg()`/`commit`/`feed`→`SessionEvent`/`reveal_msg`/`ack_msg`/`reconnect_msg(BoardHash)`/`reconnect_ack_msg(BoardHash)`/`abort_turn`/`peer_auth_hash`。`WireMessage` は serde（`type` タグ）。
> - **TCP 殻（第三段）** `tui/src/net.rs`（120 行）: `NetEvent{Message(WireMessage),Disconnected}`、`Connection{stream,events}` の `send(&WireMessage)`/`events`、`reader_loop`（4byte 長さ＋serde）。**この段で `NetEvent` に System 枝を足し、WS 殻を別モジュールに新設する**（TCP 殻は無変更に近い）。
> - **消費側（第三段）** `tui/src/online.rs`: `run_online(terminal, config)`——接続→`ClientSession::new(config.local_side, &secret)`→`hello_msg` 送信→`wait_and_feed_hello`→メインループ（`session.feed`→`SessionEvent` 分岐・`session.commit`→reveal/ack）。reconnect は `reconnect_socket_only`（背景スレッド）＋メインループの Reconnect 交換（R1）。`OnlineConfig{local_side:Side, mode:ConnectMode, secret:Vec<u8>}`、`ConnectMode{Listen(u16),Connect(String)}`。
> - **ポータル** `tui/src/portal.rs`（489 行）: `PortalResult{Local,Online(OnlineConfig),Quit}`、`Screen{Menu{selected},OnlineForm{listen,addr_or_port,secret,focused,error}}`、`MENU_LABELS`（ローカル／先手待受／後手接続／終了）、`LastConnection{listen_port,connect_addr,secret}`、`make_form`。
> - **リリース** `.github/workflows/release.yml`: タグ `v*` push → **版番の番人**（タグ `v0.12.0` と ワークスペース `Cargo.toml` の `version` の一致を要求）→ `cargo build --release -p fukanzen-shogi-tui --target x86_64-pc-windows-msvc` → GitHub Release に `fukanzen-shogi-tui-0.12.0-windows-x64.exe`。**windows-msvc ビルドなので WS の TLS は rustls を選ぶ**。
> 関連文書: `不完全将棋_実装指示書_通信核の一本化アーク_概観と段組`、第一〜三段指示書、`archive/implementation/不完全将棋_実装指示書_ブラウザ秘匿対戦_DurableObject`（DO 中継の由来）、`design/不完全将棋_版図_世界観と設計方針`。
> 性格: 第四段は**「TUI に WebSocket 殻を足し、DO の部屋へ入って対局する。side は DO が告げ、対局チャネルは同じ `ClientSession` で駆動、再接続は DO の枠組みに乗せる」**。アークの結実——TUI ↔ ブラウザが同じ部屋に座る。**同期 `tungstenite`＋rustls** を net.rs の reader スレッド構造に噛ませる（async 不要）。LAN は無変更で併存。挙動の急所は「side を DO から受けてから session を作る」順序と、WS 切断→WS 再接続→`you_reconnected`→`reconnect_msg` の再接続経路。**先手の spectate/record 責務はこの段では作らない**（§0）。締めで `Cargo.toml` を 0.12.0 に上げ、タグ v0.12.0 で Windows バイナリを焼く。

---

## 0. 目的と範囲

- **作るもの（内部順序：まず対局、次に再接続）**:
  1. **WS 殻**（§1）: 新モジュール `tui/src/net_ws.rs`。同期 `tungstenite`＋rustls で `wss://…/room/<部屋キー>` へ接続。reader スレッドが WS フレームを読み、game channel は `NetEvent::Message(WireMessage)`、DO システムは `NetEvent::System(DoSystemMsg)`、切断は `NetEvent::Disconnected` に分類。`send(&WireMessage)` と `send_raw(&str)`（`request_reset` 等の DO 制御用）。
  2. **`NetEvent` 拡張**（§2）: `net.rs` の `NetEvent` に `System(DoSystemMsg)` を追加。`DoSystemMsg{SideAssigned{side:Side}, RoomFull, PeerDisconnected, PeerReconnected, YouReconnected}`。TCP 殻は System を出さない。
  3. **トランスポート抽象**（§3）: TCP `Connection` と WS 接続を共通 API（`send(&WireMessage)`＋`events:&Receiver<NetEvent>`）で扱えるようにする（trait か enum）。メインループの対局駆動を両殻で共有。
  4. **ポータル拡張**（§4）: メニューに「通信対戦（クラウド・部屋キー）」を追加。クラウド用フォーム（部屋キー＋secret・**side 選択なし**）。`ConnectMode::Cloud{room_key:String}`。
  5. **run_online のクラウド経路**（§5）: `ConnectMode` で分岐。Cloud は WS 接続 → `System(SideAssigned{side})` を待って `ClientSession::new(side, &secret)` → `hello_msg` 送信 → 共有メインループ。System イベント（`YouReconnected`→`reconnect_msg`（＋必要なら `request_reset`）、`PeerDisconnected`/`PeerReconnected`/`RoomFull`）を捌く。
  6. **依存とビルド**（§6）: `tui/Cargo.toml` に `tungstenite`（rustls feature）。windows-msvc で通ること。
  7. **版とリリース**（§7）: ワークスペース `Cargo.toml` を `0.12.0` へ。タグ v0.12.0。
- **位置づけ**: 通信核の一本化アークの**結実**。TUI と web が同じ核・同じ DO・同じ部屋を共有する。
- **作らないもの（＝理由つき）**:
  - **先手の spectate/record 責務**: `spectate_meta`/`spectate_turn`/`spectate_result` の送出と、記録係の招待/受諾フロー。これらは先手だけが担い、生観戦・サーバアーカイブ・記録係の綴じに関わる。**この段では作らない**——記録係の道はバックログ §A の独立した畝で、一本化アークに引き込むと記録係アークと結合して重くなる（過ぎたるは及ばざる）。**既知の限界**として v0.12.0 のリリースノート／バックログに明記: 「TUI が先手のクラウド対局は、まだ生観戦・アーカイブされない」。対局自体は成立する（game channel は DO が relay する）。→ 別畝 4b で対応。
  - **LAN（TCP）経路の変更**: `Listen`/`Connect` と第三段の駆動は無変更。WS は足すだけ。
  - **async ランタイム**（tokio 等）: 同期 tungstenite＋スレッドで足りる（net.rs の reader_loop と同じ構造）。過ぎたるは及ばざる。
  - **観戦 `/watch` クライアントの TUI 実装**: 別畝。
  - **サーバ（DO）の変更**: TUI は web と同じ部屋プロトコルに乗るだけ。server/ は無変更。

---

## 1. `tui/src/net_ws.rs` — WebSocket 殻（同期 tungstenite＋rustls）

net.rs の TCP 殻と**同じ公開 API**（`send(&WireMessage)`＋`events: Receiver<NetEvent>`）を、WS の上で実装する。

- **接続**: `WsConnection::connect(server_url: &str, room_key: &str) -> Result<Self, WsError>`。URL は `format!("{}/room/{}", server_url, urlencode(room_key))`（server_url の既定は定数 `wss://fukanzen-shogi-ws.tokuhira.workers.dev`）。`tungstenite::connect`（rustls）で WS ハンドシェイクまで確立。
- **reader スレッド**: TCP の `reader_loop` に相当。`read_message()` ループで:
  - `Message::Text(s)` → `classify(&s)`:
    - まず DO システム type か判定（`peer_joined`/`room_ready`/`room_full`/`peer_disconnected`/`peer_reconnected`/`you_reconnected`）→ `NetEvent::System(DoSystemMsg::…)` に変換（`peer_joined`/`room_ready` は `your_side` を読み `SideAssigned{side}`）。
    - それ以外は `WireMessage::from_json(&s)`（game channel）→ 成功なら `NetEvent::Message(wire)`、失敗（`UnknownType`/`InvalidJson`）は**無視**（spectate_*/record_* 等の未対応 DO メッセージが来ても落とさない。online.js が game channel 前に捌くのと同じ精神）。
  - `Message::Ping` → 自動 `Pong`（tungstenite が自動処理するなら任せる。しないなら明示送信）。
  - `Message::Close` / 読み取りエラー → `NetEvent::Disconnected` を送って終了。
- **送信**: `send(&mut self, msg: &WireMessage) -> Result<(),WsError>`＝`self.ws.write_message(Message::Text(msg.to_json()))`。`send_raw(&mut self, json: &str)`＝DO 制御メッセージ（`request_reset` 等）を素の Text で送る。
- **注意（tungstenite の書き込み共有）**: reader スレッドと送信が同じ WebSocket を触るので、tungstenite の `WebSocket<MaybeTlsStream>` を `Arc<Mutex<>>` で包むか、`stream` を try_clone できない TLS の制約に合わせ「reader スレッドが read 専用・送信はメインが write 専用」を安全に分ける設計にする（TCP の `try_clone` に相当する分割）。tungstenite は分割 API が限られるので、**Arc<Mutex<WebSocket>>** を reader/writer で共有し、read はブロッキング read をスレッドで、write はメインで Mutex ロックして書く形が素直。実装者判断で最小の安全形を選ぶこと（デッドロックしないよう read はロックを長く持たない）。
- **WsError**: 接続失敗・TLS 失敗・URL 不正を表す小さな型（online.rs でエラー表示に整形）。

**受け入れ（§1 単独）**: `WsConnection` が `/room/<key>` へ繋ぎ、game channel の `WireMessage` と DO システムを `NetEvent` に分類して流す。TCP 殻と同じ `send`/`events` を持つ。

## 2. `net.rs` — `NetEvent` に System 枝

```rust
pub enum NetEvent {
    Message(WireMessage),
    System(DoSystemMsg),   // WS 殻のみが出す（TCP 殻は出さない）
    Disconnected,
}

/// DO の部屋・システムメッセージ（対局チャネル外）。WS 殻が分類して surface する。
pub enum DoSystemMsg {
    SideAssigned { side: engine::types::Side }, // peer_joined / room_ready の your_side
    RoomFull,
    PeerDisconnected,
    PeerReconnected,
    YouReconnected,
}
```

- TCP の `reader_loop` は `System` を生成しない（LAN に部屋概念はない）。
- online.rs の LAN 経路は `System(_)` を無視（来ないが match の網羅のため `_ => {}`）。

## 3. トランスポート抽象（両殻で対局ループを共有）

TCP `Connection` と `WsConnection` を共通に扱う。最小形は trait:

```rust
pub trait GameTransport {
    fn send(&mut self, msg: &WireMessage) -> io::Result<()>;
    fn events(&self) -> &std::sync::mpsc::Receiver<NetEvent>;
}
impl GameTransport for Connection { /* TCP */ }
impl GameTransport for WsConnection { /* WS。send_raw は WS 固有として別に持つ */ }
```

- run_online のメインループ（`session.feed`→`SessionEvent` 分岐、`session.commit`→reveal/ack）は `&mut dyn GameTransport` の上で回す＝**TCP/WS 共有**。
- `send_raw`（`request_reset`）は WS 固有。クラウド経路だけが使うので、トレイト外の WS 専用メソッドとして持ち、クラウド分岐から呼ぶ（LAN 経路は触れない）。
- ※enum `Transport{Tcp(Connection),Ws(WsConnection)}` で dispatch する形でも可。trait と enum のどちらが素直かは実装者判断（過ぎたるは及ばざる——`dyn` で足りるなら trait、所有権が絡むなら enum）。

## 4. `portal.rs` — クラウド対戦の追加

- `MENU_LABELS` に「通信対戦（クラウド・部屋キー）」を追加（既存: ローカル／先手待受／後手接続／終了 の間に挿す）。
- `Screen` に `CloudForm{ room_key:String, secret:String, focused:usize, error:Option<String> }` を追加（**side 選択なし**——DO が告げる）。既存 `OnlineForm`（LAN）は無変更。
- `ConnectMode` に `Cloud{ room_key:String }` を追加（server_url は定数。将来変えたくなったら足す）。
- `OnlineConfig.local_side` の扱い: **クラウドでは接続時に未確定**。最小改修として `OnlineConfig` の `local_side` を LAN 専用の初期値のまま持たせ、**run_online のクラウド分岐が DO の `SideAssigned` で上書き**する（`local_side` を `Option<Side>` に変える手もあるが、LAN 経路の変更が波及するので、クラウド分岐でローカル変数として side を確定させる方が影響が小さい。実装者判断）。
- `LastConnection` にクラウド部屋キーの記憶を足す（`room_key: String`）と二局目以降が楽（任意）。
- 検証卓（Local）・LAN フォームの挙動は無変更。

## 5. `online.rs` — クラウド経路（side は DO から・再接続は DO 枠組み）

`run_online` を `config.mode` で分岐する。**LAN 経路（Listen/Connect）は第三段のまま**。**Cloud 経路**を足す。

### 5.1 接続とハンドシェイク（side を DO から）

```
Cloud{room_key} の場合:
  1. WsConnection::connect(SERVER_URL, &room_key)  // /room/<key>
  2. SideAssigned{side} を待つ（System イベント。room_full なら「満室」でポータルへ）
       - タイムアウト（例 30s）で「相手を待っています…」表示を挟んでよい
  3. let mut session = ClientSession::new(side, &config.secret);
  4. transport.send(&session.hello_msg())?;         // Hello を DO 経由で相手へ
  5. 相手の Hello を feed して HandshakeDone を待つ（wait_and_feed_hello 相当。
     ただし System イベントも来るので、Hello 以外の System を捌きつつ待つ）
```

- **side は portal でなく DO**。`peer_joined`（先に入った側＝先手）/`room_ready`（後に入った側＝後手）の `your_side` を WS 殻が `SideAssigned{side}` にして渡す。この順序（side 確定 → session 構築 → hello）を守る。
- 版交渉は第三段同様 `feed(Hello)` の中（`SessionError::VersionMismatch`）。

### 5.2 対局ループ（共有）

第三段の `session.feed`→`SessionEvent` 分岐・`session.commit`→reveal/ack を**そのまま**共有トランスポート上で回す。差分は「`NetEvent::System(_)` も届く」点だけ——ループで System を捌く（§5.3）。

### 5.3 System イベントの処理（クラウド固有）

```
NetEvent::System(sys) => match sys {
    DoSystemMsg::PeerDisconnected => app.message = "相手が切断しました。再接続を待っています…",
    DoSystemMsg::YouReconnected => {
        // 自分が再接続した（WS 再確立後、DO が告げる）。現局面で reconnect を送る。
        let bh = board_hash(&kifu.current());
        let _ = transport.send(&session.reconnect_msg(bh));
        // web は同時に request_reset を送る枝がある（online.js 行 290）。
        // 挙動を揃えるなら transport.send_raw(r#"{"type":"request_reset"}"#) を送る。
        // 送るか否かは実 DO 検証で決める（送らずに再開できるならより素直）。
    }
    DoSystemMsg::PeerReconnected => app.message = "相手が再接続しました。",
    DoSystemMsg::RoomFull => { /* 満室 → Aborted しポータルへ */ }
    DoSystemMsg::SideAssigned{..} => { /* ハンドシェイク後は無視 */ }
}
```

- 相手からの `WireMessage::Reconnect`/`ReconnectAck` は §5.2 の feed 分岐（`PeerReconnectRequest`/`ReconnectAck`）で第三段どおり処理。**本人照合は核・再開点は `find_resume_point`**（第三段で確立済みを再利用）。`IdentityMismatch` も第三段どおり Abort。

### 5.4 クラウド再接続の全体像（WS 切断時）

```
NetEvent::Disconnected（Cloud）:
  1. session.abort_turn()（進行中ターンを捨てる。第三段と同じ）
  2. 背景スレッドで WS を同じ部屋へ再接続（WsConnection::connect のリトライ）
  3. 再確立したら transport を差し替え。DO が SideAssigned は再送しない前提だが、
     再接続時は you_reconnected/peer_reconnected が来る（web と同じ）。
  4. YouReconnected（§5.3）で reconnect_msg を送り、以降は feed 分岐で再開。
```

- LAN の `reconnect_socket_only`（TCP）と対をなす「WS 再確立のみの背景スレッド」を用意する。Reconnect 交換自体はメインループ（永続 session）で駆動する R1 を踏襲。
- **web の再接続フロー（online.js 行 278–312）を手本に**——`you_reconnected`→自分が `reconnect` 送信、`peer_reconnected`→残留側が相手の reconnect を待って ack、を TUI でも同じ順序で。

## 6. 依存とビルド

- `tui/Cargo.toml` に WS クライアントを追加。**同期・rustls**:
  ```toml
  tungstenite = { version = "0.x", default-features = false, features = ["rustls-tls-webpki-roots"] }
  ```
  （正確な feature 名は採用版で確認。要点は「native-tls/OpenSSL を引かず rustls で TLS」。webpki-roots か native-roots かは実装者判断——windows-msvc で確実に通る方を。）
- `cargo build --release -p fukanzen-shogi-tui --target x86_64-pc-windows-msvc` が通ること（release.yml が焼く対象）。CI（ci.yml）の clippy `-D warnings` も通すこと。
- web・server・protocol は無変更。

## 7. 版とリリース

- ワークスペース `Cargo.toml` の `version` を **`0.12.0`** へ（release.yml の版番の番人がタグと照合する）。クラウド参加＝利用者に見える新能力＝マイナー bump。
- `--version`（`fukanzen-shogi-tui.exe --version`）が 0.12.0 を返すこと（release ノートが確認手段に挙げている）。
- リリース手順（作り手が実行）: 第四段をコミット（`Cargo.toml` 0.12.0 込み）→ `git tag v0.12.0` → `git push origin v0.12.0`。release.yml が Windows バイナリを焼き GitHub Release を作る。
- web `?v=` は無関係（この段は TUI のみ）。

## 8. テスト・受け入れ

- **段階的検証（実 DO）**:
  1. `cargo build -p fukanzen-shogi-tui`（と windows-msvc target）通過・clippy 警告なし。
  2. **TUI ↔ ブラウザ**: 同じ部屋キーで、一方をブラウザ・一方を TUI で入室。side が DO から割り当てられ（先入り=先手）、hello 交換→複数手→通常終局／投了。**TUI が後手のとき**は特に通しで（先手ブラウザが spectate/record を担うので観戦・アーカイブも動く）。
  3. **TUI ↔ TUI（クラウド）**: 両方 TUI で同じ部屋。対局成立（ただし先手 TUI は spectate/record を送らないので観戦・アーカイブは無い＝既知の限界）。
  4. **版不一致**: 旧版 TUI（PROTOCOL 4）と新 TUI/ブラウザで、`feed(Hello)` の版交渉が弾く。
  5. **クラウド再接続**: 対局中に TUI の WS を落とし、同じ部屋へ再接続→`you_reconnected`→`reconnect_msg`→再開点一致で resume。別 secret で `IdentityMismatch`→Abort。
  6. **LAN 回帰**: 第三段の LAN 自己対戦（通常＋再接続）が無傷。
- **受け入れ条件**:
  - TUI が `/room/<key>` へ入り、DO が告げた side で `ClientSession` を構築し、ブラウザと対局・再接続できる。
  - 対局チャネルは同じ `ClientSession`/`WireMessage`（TCP と WS で共有）。
  - LAN（TCP）は無変更で併存。web・server・protocol 無変更。
  - `Cargo.toml` 0.12.0、`--version` が一致。
  - 先手の spectate/record は未実装（既知の限界として明記）。
- **リリース確認**: タグ v0.12.0 push で release.yml が緑、`fukanzen-shogi-tui-0.12.0-windows-x64.exe` が Release に上がる。

## 末尾要約

TUI に同期 `tungstenite`＋rustls の WebSocket 殻（`net_ws.rs`）を足し、`wss://…/room/<部屋キー>` で DO の部屋へ入る。`NetEvent` に `System(DoSystemMsg)` を足し、WS 殻が DO の部屋メッセージ（side 割り当て・満室・切断・再接続）を分類する。トランスポートを抽象化して対局ループ（`ClientSession` 駆動）を TCP/WS で共有。side は portal でなく DO の `peer_joined`/`room_ready` が告げ、それを受けてから session を構築し hello を送る。再接続は DO の `you_reconnected`/`peer_reconnected` に乗せ、第三段の session 再接続機構（reconnect_msg／find_resume_point）を再利用する。**先手の spectate/record 責務はこの段では作らない**（既知の限界・別畝 4b）。LAN は無変更で併存。`Cargo.toml` を 0.12.0 に上げ、タグ v0.12.0 で Windows バイナリを焼く。TUI ↔ ブラウザが同じ部屋に座る——アークの結実。

## 不変の原則

- **核と交換可能な殻**: 同じ `ClientSession`/`WireMessage` を、TCP でも WS でも駆動する。殻だけが違う。
- **side は DO が告げる**: クラウドでは listen/connect の選択が消え、`peer_joined`/`room_ready` の `your_side` が陣営を決める。side 確定 → session 構築 → hello の順を守る。
- **審判なし・relay 透明**: DO は commit-reveal を裁定せず素通し。TUI も自分で相手の reveal を検証する（`ClientSession` の検証は不変）。
- **再接続は核＋殻**: 本人照合は核（auth_hash）、再開点は殻（find_resume_point）、DO 枠組み（you_reconnected/peer_reconnected）に乗せる。第三段の機構を再利用。
- **過ぎたるは及ばざる**: async ランタイムを入れない（同期＋スレッド）。先手の spectate/record を引き込まない（記録係アークへ）。トランスポート抽象は `dyn`/enum の素直な方で足りる。
- **LAN 併存**: TCP 経路は無変更。WS は足すだけ。
