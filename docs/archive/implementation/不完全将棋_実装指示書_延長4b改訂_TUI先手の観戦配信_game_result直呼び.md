# 不完全将棋 実装指示書 — 延長 4b（改訂版）：TUI 先手の観戦配信（`game_result` 直呼び版）

> 対象実行者: Claude Code（Sonnet 5）
> 前提: 終局判定の単一正本化アーク着地＋セキュリティ修正（HEAD `9a6e35b`・配布 v0.12.2）。**この改訂版が初版を差し替える**——初版は結果語彙を手組みの対応表で書いていたが（`DrawReason::MaxTurns` を仮定した綻びを含んでいた）、いま `protocol::game_result` が単一正本として在るので、`spectate_result` は **`game_result` を直接呼ぶだけ**になる（max_turns も投了も正しく出る）。
> この段は、TUI が**先手**でクラウド対局するとき観戦配信（`spectate_meta`/`spectate_turn`/`spectate_result`）を送り、その対局を **/watch の観戦者が生で見られる**ようにする。**記録係（永続書庫・二証人）は作らない**（クラウド主導で Web/TUI 双方を見据える別アーク・バックログ §A）。ビルド・実 DO 検証は Sonnet 側。締めで配布パッチ bump（v0.12.3 推奨）。
> 関連する現物（すべて実地で確認済み・HEAD `9a6e35b` 基準）:
> - **手本＝web の先手観戦配信** `web/online.js`: `spectate_meta`（`{"type":"spectate_meta","version":<version_tuple object>,"initial_sfen":<SFEN>}`・先手のみ・握手直後一度）、`spectate_turn`（`{"type":"spectate_turn","s":<sente_usi>,"g":<gote_usi>}`・先手のみ・各手両者公開後）、`spectate_result`（`{"type":"spectate_result","kind":<kind>,"outcome":<outcome>}`・先手のみ・終局時一度）。version_tuple の形: `{"rule":"{major}.{minor}","protocol":{n},"app":"{CARGO_PKG_VERSION}"}`。
> - **単一正本（アーク着地）** `protocol::game_result(&Kifu) -> Option<(ResultKind, Outcome)>`（投了＋盤面終局・max_turns 込み）。online.rs は既に呼んでいる（`resolve_completed_turn` の投了枝・549 行）。`engine::archive::{ResultKind, Outcome}` はいずれも `pub fn to_str(&self) -> &'static str`。
> - **送出経路** `tui/src/net_ws.rs`: `WsConnection::send_raw(&str)`（122 行・現在 `#[allow(dead_code)]`。この段で活性化）。`tui/src/online.rs`: `enum Transport { Tcp(Connection), Ws(WsConnection) }`（59 行）の `send(&WireMessage)`/`events`。**`send_control` は未実装＝この段で足す**。
> - **送出点（現 HEAD の行）**:
>   - ハンドシェイク完了後: `side` 確定 → 接続完了メッセージ（158 行付近）→ `let mut kifu = Kifu::new(Position::initial());`（168 行付近）。ここが `spectate_meta` の送出点。
>   - 対局ループ内 `Ok(SessionEvent::TurnComplete { sente, gote })`（371 行）→ `resolve_completed_turn(...)`（372 行）。ここが `spectate_turn`＋（終局なら）`spectate_result` の送出点。`resolve_completed_turn` は投了・盤面終局を `game_result` 経由で `app.phase = Phase::GameOver(...)` にする。
> - **陣営とモード**: `side`（`Side::Sente`/`Gote`・クラウドは `DoSystemMsg::SideAssigned` が確定）、`config.mode`（`ConnectMode::Cloud{room_key}`・105/119 行）。
> - **import 状況** `online.rs`: `Position`・`Kifu`・`Action`・`Side` あり。`position_to_sfen` は**未 import**（`engine::serialize::position_to_sfen` を足す）。`MY_VERSION`/`game_result` は `protocol::…` 完全修飾で呼ぶ。
> 関連文書: `不完全将棋_終局判定の単一正本化アーク_概観と段組`、`不完全将棋_実装指示書_通信核の一本化_第四段_TUIにWS殻を足すクラウド参加`、`design/不完全将棋_棋譜対局データ設計_方針`（淀川・公開組手のブロードキャスト §2）。
> 性格: 4b（改訂）は**「TUI が先手でクラウド対局するとき `spectate_meta`/`spectate_turn`/`spectate_result` を送り、対局を /watch の観戦者に生配信する」**。境界が明確な小さな畝。**先手のみ・クラウドのみ**（LAN に観戦者はいない・`send_raw` は WS 固有）。秘匿を破らない（各手は両者公開後に送る）。**`spectate_result` は `game_result` を直接呼ぶ**（手組みの対応表を作らない）。記録係は作らない。web・server・protocol・engine は無変更。

---

## 0. 目的と範囲

- **作るもの**:
  1. **`Transport::send_control`**（§1）: 制御メッセージ送信（Tcp は no-op、Ws は `send_raw`）。`net_ws.rs` の `send_raw` の `#[allow(dead_code)]` を外す。
  2. **観戦配信の送出**（§2, online.rs）: クラウド先手のとき、握手後に `spectate_meta`、各 `TurnComplete` 後に `spectate_turn`、終局時に `spectate_result`（**`game_result` 直呼び**）を送る。
  3. **版**（§3）: パッチ bump（v0.12.3 推奨）。
- **位置づけ**: 通信核の一本化アークの延長の締め。第四段で開いた「TUI 先手のクラウド対局が観戦されない」限界の**観戦の穴を塞ぐ**。アーカイブ（記録係）は別アーク。
- **作らないもの（＝理由つき）**:
  - **結果語彙の手組みマッピング**: 初版の `game_over_to_archive` 表は作らない。`spectate_result` は `protocol::game_result(&kifu)` を直接呼ぶ（単一正本・max_turns/投了込み）。
  - **記録係のクラウド参加**（`record_*`・永続書庫・二証人・正準本文の一致）: クラウド主導で Web/TUI 双方を見据える別アーク（§A）。web の記録係招待 prompt 相当も作らない。
  - **LAN 経路への配信**: LAN に観戦者・DO はいない。送出は `ConnectMode::Cloud` かつ先手のときだけ。
  - **後手側の変更・TUI 観戦クライアント（/watch を見る側）**: 別。この段は「先手が配信する側」だけ。
  - **engine・protocol・server・web の変更**: 無変更。

---

## 1. `Transport::send_control`（制御送信・Tcp は no-op）

観戦の送出点（§2 の `TurnComplete`）は LAN/クラウド共有のループ内にある。LAN では送らないので Transport に制御送信を足し **Tcp は no-op**にする（`broadcasting` が false なので呼ばれないが、網羅と安全のため）:

```rust
impl Transport {
    fn send_control(&mut self, json: &str) -> io::Result<()> {
        match self {
            Transport::Tcp(_) => Ok(()),          // LAN に観戦なし（no-op）
            Transport::Ws(w) => w.send_raw(json),
        }
    }
}
```

- `net_ws.rs` の `WsConnection::send_raw` の `#[allow(dead_code)]`（121 行）を外す。

## 2. 観戦配信の送出（online.rs・クラウド先手のみ）

**判定フラグ**を一つ持つ（`side` 確定後・ループ前に）:

```rust
let broadcasting = matches!(config.mode, ConnectMode::Cloud { .. }) && side == Side::Sente;
```

すべて `transport.send_control(&json)` で送る。

### 2.1 `spectate_meta`（握手完了直後・一度）

`let mut kifu = Kifu::new(Position::initial());`（168 行付近）の直後、`broadcasting` なら:

```rust
if broadcasting {
    let ver = format!(
        r#"{{"rule":"{}.{}","protocol":{},"app":"{}"}}"#,
        protocol::MY_VERSION.rule.0,
        protocol::MY_VERSION.rule.1,
        protocol::MY_VERSION.protocol,
        env!("CARGO_PKG_VERSION")
    );
    let initial_sfen = engine::serialize::position_to_sfen(&Position::initial());
    let _ = transport.send_control(&format!(
        r#"{{"type":"spectate_meta","version":{},"initial_sfen":"{}"}}"#,
        ver, initial_sfen
    ));
}
```

- `version` の**形**（`rule`/`protocol`/`app`）は web の `version_tuple()` に揃える（観戦者がパースするため）。値は TUI 自身（`app` は tui の `CARGO_PKG_VERSION`）。
- `engine::serialize::position_to_sfen` の import を足す（online.rs は未 import）。`kifu` は初期局面から始まるので `Position::initial()` で足りる。

### 2.2 `spectate_turn`＋`spectate_result`（`TurnComplete` 内）

`Ok(SessionEvent::TurnComplete { sente, gote })`（371 行）の処理を次の順に:

```rust
Ok(SessionEvent::TurnComplete { sente, gote }) => {
    // (a) この手を観戦者へ（両者公開後なので秘匿を破らない）。投了手も送る（web と同順）。
    if broadcasting {
        let _ = transport.send_control(&format!(
            r#"{{"type":"spectate_turn","s":"{}","g":"{}"}}"#,
            sente.to_usi(), gote.to_usi()
        ));
    }
    // (b) 従来どおり解決（投了・盤面終局を game_result 経由で GameOver に）。
    resolve_completed_turn(sente, gote, &mut app, &mut kifu, &mut online_phase, side);
    // (c) 終局したら結果を観戦者へ。単一正本 game_result を直接呼ぶ（手組みの表を作らない）。
    if broadcasting {
        if matches!(app.phase, Phase::GameOver(_)) {
            if let Some((kind, outcome)) = protocol::game_result(&kifu) {
                let _ = transport.send_control(&format!(
                    r#"{{"type":"spectate_result","kind":"{}","outcome":"{}"}}"#,
                    kind.to_str(), outcome.to_str()
                ));
            }
        }
    }
}
```

- **順序**: 手（`spectate_turn`）→ 解決 → 終局なら結果（`spectate_result`）。web（`_completeTurn`→`endOnlineGame`）と同順。
- **`spectate_result` は `game_result(&kifu)` 直呼び**。`resolve_completed_turn` が投了組手も param `kifu` に積む（540 行付近）ので、`game_result(&kifu)` が投了も盤面終局も max_turns も正しく返す。`to_str()` でアーカイブ語彙の文字列に。**初版の手組みマッピングは不要**。
- 投了手のとき `sente.to_usi()`/`gote.to_usi()` は `"resign"` になる（web も投了手を `spectate_turn` で送る）。

## 3. ビルド・テスト・受け入れ・版

- **段階的検証（実 DO）**:
  1. `cargo build -p fukanzen-shogi-tui`（と windows-msvc target）・`cargo clippy -D warnings` 通過。
  2. **TUI 先手 × 観戦**: TUI を先手でクラウド入室、第三のブラウザで `/watch`。初期局面・各手・終局結果が観戦者に見えること。`spectate_meta` の version が妥当に表示されること。
  3. **各終局での結果**: 詰み・玉取り・相討ち・千日手・**最長手数500組手**・投了で、`spectate_result` の kind/outcome が正しい（`game_result` 経由なので web と一致・max_turns も出る）。
  4. **秘匿の非破壊**: 観戦者に commit/reveal が見えず、各手は両者公開後にのみ現れる。
  5. **後手・LAN の無変更**: TUI 後手のクラウド対局は従来どおり（観戦を送らない）。LAN 自己対戦（通常＋再接続）が無傷（`broadcasting` が false で発火しない）。
- **受け入れ条件**:
  - クラウド先手の TUI 対局が /watch で生観戦できる（meta→turn×N→result）。
  - `spectate_result` が `game_result` 直呼びで、手組みの結果表が無い。max_turns 終局でも正しく配信される。
  - LAN・後手・記録係は無変更（記録係は未実装のまま＝アーカイブは別アーク）。
  - engine・protocol・server・web に差分ゼロ。`Cargo.toml` とタグが一致。
- **版**: 観戦配信の小増分。**配布パッチ bump（v0.12.3 推奨）**。`Cargo.toml` を上げ `--version` を揃える。

## 末尾要約

TUI が先手でクラウド対局するとき、`spectate_meta`（版＋初期局面・握手後一度）・`spectate_turn`（各手完了後・両者公開済み）・`spectate_result`（終局時・**`protocol::game_result` を直接呼ぶ**）を `send_raw` 経由で送り、/watch の観戦者に生配信する。送出は `ConnectMode::Cloud` かつ先手のときだけ。`Transport::send_control`（Tcp は no-op）を足し `send_raw` を活性化する。初版の手組み結果マッピングは、単一正本 `game_result` の直呼びに置き換わり不要になった（max_turns も正しく出る）。**記録係（永続書庫）は作らない**——クラウド主導で Web/TUI 双方を見据える別アークへ。LAN・後手・engine・protocol・server・web は無変更。パッチ bump（v0.12.3）で延長を締める。

## 不変の原則

- **結果は単一正本から**: `spectate_result` は `protocol::game_result` を直接呼ぶ。手組みの結果表を作らない（アークの果実）。
- **先手のみ・両者公開後**: 観戦配信は先手が担い、各手は commit/reveal 完了後にだけ送る（秘匿境界を破らない・淀川 §2）。
- **クラウドのみ**: LAN に観戦者はいない。`ConnectMode::Cloud` かつ先手でゲート。`send_control` は Tcp で no-op。
- **記録係は引き込まない**: 永続書庫・二証人・正準本文の一致は別アーク。この段は観戦の穴だけを塞ぐ。
- **無変更の広さ**: engine・protocol・server・web・LAN・後手に触れない。触るのは online.rs（送出）と net_ws.rs（dead_code 解除）のみ。
