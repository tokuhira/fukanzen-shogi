# 不完全将棋 実装指示書 — 終局判定の単一正本化 Step B：protocol に `game_result` を新設

> 対象実行者: Claude Code（Sonnet 5）
> 前提: Step A 着地（HEAD `01dc5fc`。engine に `terminal_to_result(&Terminal)->Option<(ResultKind,Outcome)>` が据わった・evaluate は無変更・盤面終局限定）。この段は **protocol に「投了と盤面終局を合流させる単一の窓口」`game_result` を新設**する。投了はプロトコルの範疇（本将棋/USI どおり・アーク概観 §1）——protocol が投了を先に判定し、盤面終局は engine の `evaluate`＋`terminal_to_result` へ委譲する。純粋な追加のみ、他クレートは変更しない。`cargo test -p protocol` で完結する。
> 関連する現物（すべて実地で確認済み・HEAD `01dc5fc` 基準）:
> - `protocol/Cargo.toml`: `engine = { path = "../engine" }`（protocol は engine に依存・evaluate/terminal_to_result/archive を呼べる）。
> - `engine/src/terminate.rs`: `pub fn evaluate(kifu: &Kifu) -> Terminal`（盤面終局。**最後の組手が投了の kifu を渡すと panic**——Step A で doc 明記済み）、`pub fn terminal_to_result(&Terminal) -> Option<(ResultKind, Outcome)>`（Ongoing は None）。
> - `engine/src/archive.rs`: `ResultKind{…,Resign,…}`・`Outcome{SenteWins,GoteWins,Draw,None}`。
> - `engine/src/kifu.rs`: `pub struct Kifu { pub plies: Vec<Ply>, … }`。`Ply` は `sente: Action`・`gote: Action`（evaluate が `last.sente`/`last.gote` で使用）。
> - `engine/src/types.rs`: `pub fn is_resign(self) -> bool`（Action・208 行）。
> - **投了の勝敗規約（現物の三箇所と一致させる）**: TUI online.rs `resolve_completed_turn`（(true,true)→両者投了=引分／(true,false)先手投了→後手勝／(false,true)後手投了→先手勝）、web board.js `resultOverride`（sResign&&gResign→draw／sResign→gote/gResign→sente）。ルール v0.6 §5.4（両者同時投了→引き分け）。
> - `protocol/src/lib.rs`: 現在 `pub mod auth/client/commit/hash/negotiate/recovery/session/wire` と各 `pub use`。ここに `result` を足す。
> 関連文書: `不完全将棋_終局判定の単一正本化アーク_概観と段組`、Step A 指示書、`design/不完全将棋_ルール仕様_v0.6`（§5.4 両者投了）。
> 性格: Step B は**「投了（protocol の領分）と盤面終局（engine）を合流させる単一の窓口 `game_result(kifu)->Option<(ResultKind,Outcome)>` を protocol に置く」**。最後の組手の投了を先に判定し、投了でなければ engine の `terminal_to_result(evaluate(kifu))` へ委譲。`None` は未了（対局中）。これが以後 web（Step C）と TUI（Step D）と 4b が呼ぶ**唯一の終局判定窓口**になる。engine には投了を入れない（合流は `(ResultKind,Outcome)` のレベル）。純粋・`cargo test -p protocol` で完結。

---

## 0. 目的と範囲

- **作るもの**:
  1. `protocol/src/result.rs` — `pub fn game_result(kifu: &Kifu) -> Option<(ResultKind, Outcome)>`。投了合成の単一窓口。
  2. `protocol/src/lib.rs` — `pub mod result;` と `pub use result::game_result;`。
  3. 単体テスト（`result.rs` の `#[cfg(test)]`）: 投了三態・未了（None）・盤面終局への委譲一致。**`cargo test -p protocol` で緑**。
- **位置づけ**: 終局判定の単一正本化アークの **Step B**。以後 Step C（web）・Step D（TUI）・4b がこの窓口を呼ぶ。
- **作らないもの（＝理由つき）**:
  - **engine の変更**: 盤面終局は engine のまま。投了 variant を `Terminal` に足さない（アーク概観 §1）。
  - **engine-wasm / protocol-wasm / tui / web の変更**: Step C・D。この段は protocol への追加のみ。engine-wasm のインラインマッピングもまだ残る（Step C で置換）。
  - **`Unfinished` の使用**: 「対局中＝終局していない」は `None` で表す（`ResultKind::Unfinished` はアーカイブ済みだが未了の局を指す別用途——ここで混ぜない）。

---

## 1. `protocol/src/result.rs`

```rust
//! 終局判定の単一窓口。投了（プロトコルの範疇）と盤面終局（engine）を合流させる。
//! 投了は本将棋/USI でも着手でなく宣言なので、ここ（protocol 層）で先に捌き、
//! 盤面終局は engine の evaluate + terminal_to_result へ委譲する（アーク概観 §1-2）。

use engine::archive::{Outcome, ResultKind};
use engine::kifu::Kifu;
use engine::terminate::{evaluate, terminal_to_result};

/// 対局の終局結果。対局中（未了）は `None`。
///
/// 1. 最後の組手が投了なら、投了として勝敗を返す（盤面に依らず・投了優先）。
///    投了組手を先に捌くことで、後段の `evaluate` に投了組手を渡さない
///    （`evaluate` は投了組手で panic する＝Step A の前提）。
/// 2. 投了でなければ engine の盤面終局へ委譲（`Ongoing` は `None`）。
pub fn game_result(kifu: &Kifu) -> Option<(ResultKind, Outcome)> {
    // 1. 投了（protocol の領分）。投了は必ず最後の組手（そこで対局が終わる）。
    if let Some(last) = kifu.plies.last() {
        match (last.sente.is_resign(), last.gote.is_resign()) {
            (true, true) => return Some((ResultKind::Resign, Outcome::Draw)),      // 両者投了（v0.6 §5.4）
            (true, false) => return Some((ResultKind::Resign, Outcome::GoteWins)), // 先手投了 → 後手勝ち
            (false, true) => return Some((ResultKind::Resign, Outcome::SenteWins)),// 後手投了 → 先手勝ち
            (false, false) => {}
        }
    }
    // 2. 盤面終局は engine へ委譲（Ongoing なら None）。
    terminal_to_result(&evaluate(kifu))
}
```

- **投了優先**: 現物の三箇所（online.rs・web・app.rs local）はいずれも投了を盤面判定より先に見る。同じ順を守る。玉取りと投了が同組手で同時でも、投了は概念上の終局宣言として優先（かつ勝敗は一致するので実害なし）。
- **委譲**: 盤面終局は `terminal_to_result(evaluate(kifu))`。`Ongoing`→`None`（＝未了）がそのまま透過する。

## 2. `protocol/src/lib.rs` への配線

```rust
pub mod result;
pub use result::game_result;
```

## 3. テスト（`cargo test -p protocol` で緑）

`result.rs` の `#[cfg(test)]`:

- **投了三態**（最小の kifu を組んで最後の組手を投了に）:
  - 先手投了（`Ply{sente: Action::Resign, gote: <任意の合法手 or 何か>}` を push）→ `Some((ResultKind::Resign, Outcome::GoteWins))`。
  - 後手投了 → `Some((ResultKind::Resign, Outcome::SenteWins))`。
  - 両者投了（`Ply{Resign, Resign}`）→ `Some((ResultKind::Resign, Outcome::Draw))`。
  - ※投了組手の相手側 Action は投了判定に影響しない（`is_resign` だけ見る）。テストでは相手側に適当な `Action`（例えば合法手 or もう一方も Resign）を置く。**投了組手を含む kifu に対し `game_result` が panic しない**こと（`evaluate` へ流さず先に返るため）も確認。
- **未了**: 初期局面のみの kifu（`plies` 空）→ `evaluate` は `Ongoing` → `game_result` は `None`。
- **盤面終局への委譲一致**: 投了を含まない任意の kifu `k` について `game_result(&k) == terminal_to_result(&evaluate(&k))` が成り立つこと（委譲の透過性）。盤面終局の具体局面を組むのが重ければ、既存の engine テスト fixture か、初期局面（→ ともに `None`）での一致で最小確認する。

## 4. 受け入れ条件

- `cargo test -p protocol` 緑（投了三態・未了・委譲一致）。`cargo build` ワークスペース全体が通る。`cargo clippy -D warnings` 通過。
- `game_result` が投了を先に捌き、投了組手を含む kifu で **panic しない**（`evaluate` へ流さない）。
- 投了の勝敗が現物の三箇所（online.rs・web・app.rs）と一致（先手投了→後手勝ち等）。
- `engine`（core）・`engine-wasm`・`tui`・`web` に差分ゼロ。配布版・web `?v=` 据え置き。

## 末尾要約

protocol に `game_result(kifu: &Kifu) -> Option<(ResultKind, Outcome)>` を新設する。最後の組手の投了を先に判定し（先手投了→後手勝ち・後手投了→先手勝ち・両者投了→引き分け）、投了でなければ engine の `terminal_to_result(evaluate(kifu))` へ委譲する（`None` は未了）。投了はプロトコルの範疇なのでこの層で合成し、engine は盤面終局のまま。これが以後 web・TUI・4b が呼ぶ唯一の終局判定窓口になる。純粋な追加のみで、他クレートは触らず、`cargo test -p protocol` で完結する。

## 不変の原則

- **投了は protocol・盤面は engine**: 投了の合成は protocol の `game_result`。engine は盤面終局だけ（投了を知らない）。本将棋/USI どおり。
- **単一窓口**: 終局を知りたい者は `game_result` を呼ぶ。以後 web も TUI も同じ窓口（Step C・D）。
- **投了優先・投了は最後の組手**: 盤面判定より先に投了を見る（現物の三箇所と同順）。投了組手を `evaluate` へ流さない（Step A の前提を守る）。
- **未了は `None`**: 対局中は `None`。`Unfinished` と混ぜない。
- **純粋な追加**: engine-wasm のインラインマッピングは Step C まで残る。この段は protocol への追加のみ。
