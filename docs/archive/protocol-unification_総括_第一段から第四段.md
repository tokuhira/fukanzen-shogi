# 不完全将棋 通信核の一本化アーク 総括（第一段〜第四段）

*この文書は、TUI（LAN・TCP）と web（ブラウザ・DO）が別々に持っていたワイヤ語彙とセッション進行ロジックを `protocol` 核へ一本化し、最終的に TUI がクラウド（Cloudflare Durable Object）の部屋へ入って web と同じ盤へ座れるようにした一連の実装（第一段〜第四段、配布 v0.11.2→v0.12.0）の総括である。バックログの「現在地」が持っていた完了記録をここへ綴じる。段ごとの詳細は各実装指示書（`archive/implementation/protocol-unification/`）にあり、ここは道のり・確立した設計・残された既知の限界を粗く俯瞰する地図。*

---

## 0. このアークが始まった地点と、達成したこと

**始点**: TUI は純粋な P2P（`listen`/`connect` の TCP、commit-reveal を `tui/src/net.rs` の手書き `NetMessage` と `tui/src/online.rs` の inline orchestration で駆動）。web はスター型（Cloudflare DO 中継、`protocol-wasm/src/lib.rs` の `ProtocolSession` が JSON 文字列を手組みして orchestration も担う）。純粋状態機械 `protocol::session::TurnSession` だけは共有されていたが、その上のワイヤ語彙（層B）とセッション進行（層C）は TUI と web で別々に二重管理され、乖離していた。

**終点（現在）**: `protocol` クレートに `WireMessage`（serde 正本のワイヤ語彙）と `ClientSession`（transport 非依存のセッション orchestration）が新設され、TCP（LAN）でも WS（クラウド）でも同じ核が対局を駆動する。TUI は web と同じ Cloudflare DO の部屋へ入って対局できるようになった——**「核と交換可能な殻」という設計哲学が、実装として初めて TUI と web を跨いで結実した**。LAN は一切変更なく併存。PROTOCOL は 4→5、配布版は v0.11.2→**v0.12.0**（クラウド参加＝利用者に見える新能力＝マイナー bump）。

**確立した設計パターン**（次のアーク・他の殻への移植の下地）:
- **核は typed・純粋・注入で受ける**: `ClientSession` は nonce・board_hash・Action を呼び出し側から typed な値で受け取る。sfen/usi の解釈も乱数生成も核は持たない（`TurnSession` の流儀を継承）。
- **側 (side) は殻が決める**: LAN では listen/connect の選択、クラウドでは DO の `peer_joined`/`room_ready` が告げる。`ClientSession::new(side, secret)` はどちらの経路で決まった side かを知らない——「side 確定 → session 構築 → hello」の順序さえ守れば殻は自由に選べる。
- **審判なし・relay 透明**: DO も TCP も commit-reveal を裁定せず素通しする。各クライアントが相手の reveal を自分で検証する（`TurnSession` の検証は全段で不変）。
- **再接続は核＋殻の分業**: 本人照合（auth_hash 一致）は核（`ClientSession::feed(Reconnect)`）が担い、再開点の探索（`find_resume_point`）と DO・TCP それぞれのトランスポート再確立は殻が担う。生 secret はワイヤに一度も出さない（TUI の旧方式からの決定的な転換）。
- **トランスポート抽象は素直な enum で足りる**: `Transport::{Tcp(Connection), Ws(WsConnection)}` という薄い enum で、対局ループ（`session.feed`→`SessionEvent` 分岐、`session.commit`→reveal/ack）を LAN・クラウドで完全に共有できた。`dyn` トレイトのような重い抽象は不要だった。

---

## 1. 核の API（`protocol` クレート、第一段で新設・以後不変）

| 型・関数 | 役割 |
|---|---|
| `WireMessage`（`wire.rs`） | 対局チャネルの語彙。serde タグ付き enum（`Hello`/`Commit`/`Reveal`/`Ack`/`Reconnect`/`ReconnectAck`/`Abort`）。`to_json`/`from_json`。 |
| `wire::to_hex`/`from_hex32` | hex ⇄ バイト列の正本（TCP・WS 殻・protocol-wasm が寄せられる先）。 |
| `ClientSession`（`client.rs`） | セッション orchestration。`new(side,secret)`/`hello_msg()`/`commit(board_hash,action,nonce)`/`reveal_msg()`/`ack_msg()`/`feed(WireMessage)->SessionEvent`/`reconnect_msg(board_hash)`/`reconnect_ack_msg(board_hash)`/`abort_turn()`/`peer_auth_hash()`/`handshake_done()`。 |
| `SessionEvent` | `HandshakeDone`/`PeerCommitted`/`PeerCommitBuffered`/`PeerRevealed`/`PeerAcked`/`TurnComplete`/`PeerReconnectRequest`/`ReconnectAck`/`PeerAborted`。 |
| `SessionError` | `VersionMismatch`/`DuplicateHello`/`HandshakeNotDone`/`NoActiveTurn`/`Protocol`/`IdentityMismatch`/`BadHex`/`InvalidUsi`。 |
| `PROTOCOL_VERSION` | 4→5（第一段。hello 集約・auth_hash 再接続への刷新を反映）。 |

`abort_turn()` は第三段で追加された唯一の核への後追い（進行中ターンを捨てつつ `handshake_done`/`peer_auth_hash` を保持——LAN・クラウド共通の「切断時は捨てて指し直し」を支える）。

---

## 2. 段ごとの道のり（course-grained）

**第一段（`protocol` 核へ `WireMessage`/`ClientSession` を新設・PROTOCOL 5・要石）**: `protocol` クレートへの純粋な追加のみ。挙動の出典は `protocol-wasm` の `ProtocolSession`——handshake の版交渉・commit の先着バッファ・ack 完了時の turn 解放を typed で再現し、再接続だけは決定 3（本人照合を核へ）に従い作り替えた。他クレート無変更・wasm 再ビルドなし。`cargo test -p protocol` で完結（新規 13 テスト）。fmt の追いコミットが 1 本入った（ローカル CI チェックの教訓、後述）。

**第二段（`protocol-wasm` を薄いラッパへ・再接続を核照合へ）**: `ProtocolSession` を `ClientSession` の薄い `wasm_bindgen` ラッパへ痩せさせた。対局フロー（hello/commit/reveal/ack）の JS 向け契約とワイヤ・バイト列は一字一句保存。再接続だけ意図的に作り替え——JS 側の auth 照合を削除し、核が返す `peer_reconnect_rejected` を受けて online.js が相手へ abort を送る礼儀を保った。wasm を再ビルドし本番デプロイ、実 DO で二ブラウザの一局を確認。web `?v=` 0.11.11→0.11.12、配布版は据え置き。

**第三段（TUI をネイティブ `ClientSession` へ・LAN は TCP のまま）**: `tui/src/net.rs` を `WireMessage` を送受信するだけの TCP 殻へ痩せさせ（`NetMessage`・版交渉・hex ヘルパを削除）、`tui/src/online.rs` の handshake・turn loop・再接続をすべて永続 `ClientSession` 駆動へ書き換えた。これは「一続きに compile する coupled な変更」——`NetMessage` を消す以上、三つの経路が同時に動く。tmux で二つの TUI を実 TCP 対戦させ、socat（後日 socat をユーザーが導入）または自作 TCP プロキシで切断・再接続・auth 不一致・版不一致まで実地検証。TUI のワイヤが PROTOCOL 5 になり旧 TUI と LAN 非互換になるため配布版パッチ bump（v0.11.2→v0.11.3）。

**第四段（TUI に WS 殻を足す・クラウド参加・アークの結実）**: 新モジュール `tui/src/net_ws.rs`（同期 `tungstenite`+rustls）を追加し、`tui/src/online.rs` を `Transport::{Tcp,Ws}` で抽象化。side を DO の `peer_joined`/`room_ready` から受けてから `ClientSession` を構築する順序を実装し、再接続を DO の `you_reconnected`/`peer_reconnected` 枠組みに乗せて第三段の session レベル再接続機構をそのまま再利用した。実 DO（本番 Cloudflare Durable Object）で TUI↔TUI のクラウド対戦を実施——side 割り当て・一局・両者投了・切断検知・TLS 透過プロキシによる WS 切断/再接続の全経路を完走させた。配布版マイナー bump（v0.12.0、クラウド参加＝利用者に見える新能力）。

---

## 3. 既知の限界（次の畝・別アーク 4b）

- ~~**TUI が先手のクラウド対局は、まだ生観戦・アーカイブされない**~~ → **延長 4b（改訂）で解消**（配布 v0.12.3）。TUI が先手のクラウド対局のとき `spectate_meta`/`spectate_turn`/`spectate_result` を送出し、`/watch` の観戦者に生配信する。`spectate_result` は単一正本 `protocol::game_result` を直接呼ぶ（終局判定の単一正本化アークの果実）。**記録係の招待/受諾フロー（永続書庫・二証人）は引き続き作らない**——クラウド主導で Web/TUI 双方を見据える別アークへ（バックログ §A）。詳細は `archive/implementation/不完全将棋_実装指示書_延長4b改訂_TUI先手の観戦配信_game_result直呼び.md`。
- **観戦 `/watch` クライアントの TUI 実装**は範囲外のまま。
- **移植の抽象化**（DO を離れる際の四プリミティブの契約化）はこのアークでも作らなかった——二つ目の基盤が現実に必要になるまで待つ、という判断は不変。

---

## 4. このアークで効いた流儀（次の Opus・実装者へ）

- **地面を測ってから指示書**: 各段の指示書は、前段が着地した HEAD の現物（行番号・関数シグネチャ）に接地してから書かれた。第四段では実装前に tungstenite/rustls の crypto provider・room_full が HTTP 403（WS メッセージでない）という DO の実際の挙動を先に確認してから着手した。
- **実地検証を疑わない・再現性を確かめる**: 第四段のクラウド対戦検証で「commit が届かない」ように見える停止が一度発生したが、詳細ログを仕込んで再現させたところ実際は Durable Object のコールドスタート遅延であり、実装のバグではなかった。一度の観測で結論せず、ログを足して再現条件を切り分けた。
- **透過プロキシでトランスポート層だけを壊す**: LAN は生 TCP プロキシ（socat／自作 Python）、クラウドは TLS を素通しする生バイトプロキシ（SNI・証明書検証は本物のホスト名のまま）で、プロセスを再起動せず「ソケットだけ切れる」状況を作った。これによりプロセス内に保持された `session`/`kifu` が本当に再接続で活きることを、プロセス再起動という別の（もっと弱い）シナリオと区別して確認できた。
- **ローカルで CI 相当のチェックを通してからコミット**: 第一段で `cargo test` だけ確認してコミットしたところ CI の `cargo fmt --check` で落ちた。以後 `cargo fmt --all -- --check`・`cargo clippy --workspace --all-targets -- -D warnings`・`cargo test --workspace` の三点をコミット前に通す運用へ改めた。
- **版の目盛り**: 挙動保存の段（第一段・第三段の TCP framing 部分）は配布版据え置きかパッチ bump、web のみの変更（第二段）は `?v=` だけ前進、利用者に見える新能力が立った段（第四段）でマイナー bump——このアークでも「出来事＝マイナー」の原則を貫いた。

---

*核は既に共有されていた。乖離していたのは殻の二枚（ワイヤ語彙とセッション進行）だけだった。それを `protocol` 核へ寄せ、TCP と WS という交換可能な殻の向こうに同じ心臓を置いたことで、LAN の TUI とクラウドの web が同じ部屋に座れるようになった。残るは記録係アークとの合流（観戦・アーカイブの TUI 側対応）——急ぐものは何もない。*
