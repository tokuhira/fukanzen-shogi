# 不完全将棋 実装指示書 — 終局判定の単一正本化 Step A：engine に `terminal_to_result` を移設

> 対象実行者: Claude Code（Sonnet 5）
> 前提: 配布 v0.12.1。engine の `evaluate(kifu)->Terminal` が盤面終局（玉の死・詰み・千日手・最長手数500組手）を判定している。`Terminal→(ResultKind,Outcome)` のマッピングは現在 **`engine-wasm/src/lib.rs` の `evaluate_terminal` 内にインライン展開**されている。この段は、その盤面マッピングを **engine 本体へ純粋な関数として移す**だけ。他クレートは変更しない。`cargo test -p engine` で完結する。投了は engine に入れない（アーク概観 §1）。
> 関連する現物（すべて実地で確認済み）:
> - `engine/src/terminate.rs`: `pub enum Terminal { Ongoing, Loss { loser: Side, kind: LossKind }, Draw { kind: DrawKind } }`、`pub enum LossKind { Mate, KingDeath }`、`pub enum DrawKind { MutualMate, BothKingsDied, Sennichite, MaxTurns }`。`pub fn evaluate(kifu: &Kifu) -> Terminal`（step1 で最後の組手を `resolve()` して玉の死→status→sennichite→max_turns）。
> - `engine/src/archive.rs`: `pub enum ResultKind { Mate, KingDeath, SwapDraw, Sennichite, MaxTurns, Resign, Unfinished, Other }`、`pub enum Outcome { SenteWins, GoteWins, Draw, None }`（`to_str`/`from_str` あり）。**`ResultKind::Resign` は既に存在**（投了は Step B の protocol 合成で使う）。
> - **移設元＝engine-wasm の現行マッピング**（`engine-wasm/src/lib.rs` の `evaluate_terminal` 内・移設の一字一句の出典）:
>   - `Loss{Sente,Mate}` → `(Mate, GoteWins)` / `Loss{Gote,Mate}` → `(Mate, SenteWins)`
>   - `Loss{Sente,KingDeath}` → `(KingDeath, GoteWins)` / `Loss{Gote,KingDeath}` → `(KingDeath, SenteWins)`
>   - `Draw{MutualMate}` → `(Mate, Draw)` / `Draw{BothKingsDied}` → `(SwapDraw, Draw)`
>   - `Draw{Sennichite}` → `(Sennichite, Draw)` / `Draw{MaxTurns}` → `(MaxTurns, Draw)`
>   - `Ongoing` → 終局でない（`evaluate_terminal` は `{"status":"ongoing"}` を返していた）
> 関連文書: `不完全将棋_終局判定の単一正本化アーク_概観と段組`、`design/不完全将棋_ルール仕様_v0.6`。
> 性格: Step A は**「engine-wasm にインライン展開されている `Terminal→(ResultKind,Outcome)` の盤面マッピングを、engine 本体の純粋関数 `terminal_to_result` へ移す」**。盤面終局限定・投了を知らない。`evaluate` のロジックは無変更（前提を doc コメントで明記するのみ）。engine-wasm・protocol・tui・web は**この段では触らない**（engine-wasm の呼び替えは Step C）。純粋な追加なので並存しても衝突しない。`cargo test -p engine` で完結する要石。

---

## 0. 目的と範囲

- **作るもの**:
  1. `engine/src/terminate.rs` に `pub fn terminal_to_result(t: &Terminal) -> Option<(crate::archive::ResultKind, crate::archive::Outcome)>`。`Ongoing` は `None`、他は移設元の対応どおり `Some((kind, outcome))`。
  2. `evaluate` に doc コメントで前提を明記（下記 §2）。
  3. 単体テスト（`terminate.rs` の `#[cfg(test)]`）：全 Terminal variant → 期待 `(ResultKind, Outcome)`、`Ongoing` → `None`。
- **位置づけ**: 終局判定の単一正本化アークの**要石（Step A）**。Step B（protocol の `game_result`）と Step C（engine-wasm 呼び替え）がこの関数を使う。
- **作らないもの（＝理由つき）**:
  - **`evaluate` のロジック変更**: 盤面終局判定は無変更。前提の doc 明記のみ。投了の防御コードも入れない（`game_result` が投了を先に捌く＝概観 §2）。
  - **投了のマッピング**: `terminal_to_result` は盤面終局限定。投了→`(Resign, …)` は Step B の protocol 合成が担う。engine の `Terminal` に投了 variant を足さない。
  - **engine-wasm の呼び替え**: Step C。この段では engine-wasm のインラインマッピングは**そのまま残す**（並存・重複するが Step C で解消。この段は engine への追加のみ）。
  - **protocol・tui・web の変更**: それぞれ Step B・D・C。

---

## 1. `terminal_to_result`（engine::terminate）

移設元（engine-wasm の `evaluate_terminal` 内）と**一字一句同じ対応**を、typed な関数として engine に置く。

```rust
use crate::archive::{Outcome, ResultKind};
use crate::types::Side;

/// 盤面終局 `Terminal` を、アーカイブ結果語彙 `(ResultKind, Outcome)` へ写す。
/// `Ongoing` は終局でないので `None`。
///
/// この関数は**盤面終局限定**である（投了を知らない）。投了を含む勝敗の合成は
/// protocol 層の `game_result` が担う（投了はプロトコルの範疇＝アーク概観 §1）。
pub fn terminal_to_result(t: &Terminal) -> Option<(ResultKind, Outcome)> {
    Some(match t {
        Terminal::Ongoing => return None,
        Terminal::Loss { loser: Side::Sente, kind: LossKind::Mate } => (ResultKind::Mate, Outcome::GoteWins),
        Terminal::Loss { loser: Side::Gote,  kind: LossKind::Mate } => (ResultKind::Mate, Outcome::SenteWins),
        Terminal::Loss { loser: Side::Sente, kind: LossKind::KingDeath } => (ResultKind::KingDeath, Outcome::GoteWins),
        Terminal::Loss { loser: Side::Gote,  kind: LossKind::KingDeath } => (ResultKind::KingDeath, Outcome::SenteWins),
        Terminal::Draw { kind: DrawKind::MutualMate }   => (ResultKind::Mate, Outcome::Draw),
        Terminal::Draw { kind: DrawKind::BothKingsDied } => (ResultKind::SwapDraw, Outcome::Draw),
        Terminal::Draw { kind: DrawKind::Sennichite }   => (ResultKind::Sennichite, Outcome::Draw),
        Terminal::Draw { kind: DrawKind::MaxTurns }     => (ResultKind::MaxTurns, Outcome::Draw),
    })
}
```

- `import` パスは engine 内なので `crate::archive::{…}`・`crate::types::Side`。
- **網羅**: `Terminal`/`LossKind`/`DrawKind` の全組み合わせを列挙し `_ =>` を使わない（将来 variant が増えたらコンパイラが気づく）。移設元と同じ 8 対応＋Ongoing。

## 2. `evaluate` の前提を doc 明記（ロジックは無変更）

`evaluate` の doc コメントに前提を一行足す（コードは変えない）:

```rust
/// 盤面終局を判定する。**最後の組手が投了（`Action::Resign`）の kifu を渡してはならない**——
/// step1 で `resolve()` を呼ぶため panic する。投了を含む終局は protocol の `game_result` を使い、
/// そちらが投了を先に捌いてこの前提を守る（アーク概観 §2）。
pub fn evaluate(kifu: &Kifu) -> Terminal {
    // …無変更…
}
```

## 3. テスト（`cargo test -p engine` で緑）

`terminate.rs` の `#[cfg(test)]` に、`terminal_to_result` の全対応を固定:

- `Loss{Sente,Mate}` → `Some((Mate, GoteWins))`、`Loss{Gote,Mate}` → `Some((Mate, SenteWins))`。
- `Loss{Sente,KingDeath}` → `Some((KingDeath, GoteWins))`、`Loss{Gote,KingDeath}` → `Some((KingDeath, SenteWins))`。
- `Draw{MutualMate}` → `Some((Mate, Draw))`、`Draw{BothKingsDied}` → `Some((SwapDraw, Draw))`、`Draw{Sennichite}` → `Some((Sennichite, Draw))`、`Draw{MaxTurns}` → `Some((MaxTurns, Draw))`。
- `Ongoing` → `None`。
- 既存の `evaluate` テストは無傷（ロジック無変更）。

## 4. 受け入れ条件

- `cargo test -p engine` 緑（新規テスト全通過・既存無傷）。`cargo build` ワークスペース全体が通る（engine への追加が他を壊さない）。`cargo clippy -D warnings` 通過。
- `terminal_to_result` の対応が engine-wasm の現行インラインマッピングと**完全一致**（Step C で置換したとき web の結果が変わらない土台）。
- `evaluate` のロジック無変更（doc コメントのみ追加）。
- `engine-wasm`・`protocol`・`tui`・`web` に差分ゼロ。配布版・web `?v=` 据え置き。

## 末尾要約

engine-wasm にインライン展開されている盤面終局マッピング `Terminal→(ResultKind,Outcome)` を、engine 本体の純粋関数 `terminal_to_result(&Terminal)->Option<(ResultKind,Outcome)>` へ移す（Ongoing は None・移設元と一字一句一致）。`evaluate` は無変更で、「投了の組手を渡さない」前提を doc コメントに明記する。投了は engine に入れない（protocol の `game_result` が Step B で合成する）。engine への純粋な追加のみで、他クレートは触らず、`cargo test -p engine` で完結する要石。

## 不変の原則

- **engine は盤面終局だけ**: `terminal_to_result` も盤面限定。投了を知らない。`Terminal` に投了 variant を足さない。
- **共通通貨は `(ResultKind, Outcome)`**: 投了と盤面終局は result のレベルで合流する（Step B）。この段はその盤面側の写像を engine に据える。
- **移設は挙動保存**: engine-wasm の現行対応を一字一句移す。網羅列挙で variant 漏れを防ぐ。
- **並存で衝突しない**: engine-wasm のインラインマッピングはこの段で残す（Step C で `terminal_to_result` 呼び出しへ置換）。
- **過ぎたるは及ばざる**: `evaluate` に防御コードを入れない。前提の doc 明記で足りる。
