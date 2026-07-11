# 不完全将棋 実装指示書 — 終局判定の単一正本化 Step C（C-minimal）：engine-wasm を `terminal_to_result` へ寄せる

> 対象実行者: Claude Code（Sonnet 5）
> 前提: Step B 着地（HEAD `3ed622b`。engine に `terminal_to_result`、protocol に `game_result`）。**Step A で engine に据えた盤面マッピングが、engine-wasm の `evaluate_terminal` 内にインライン展開されたまま重複している**。この段（C-minimal）は、その重複を解消する——`evaluate_terminal` を `engine::terminate::terminal_to_result` 呼び出しへ置き換える。**web の挙動はバイト単位で不変**（`terminal_to_result` は Step A で現行インラインマッピングとの一致を確認済み）。web の `currentResult` の二経路（`resultOverride`＋`evaluate_terminal`）は**この段では畳まない**——投了を plies に統一するとアーカイブ正準本文が変わり、それは先送りにした記録係アーク（正準本文を Web/TUI 双方で設計する場）の領分だから（作り手判断・C-minimal）。engine-wasm だけの変更。wasm 再ビルド・web `?v=` 前進。
> 関連する現物（すべて実地で確認済み・HEAD `3ed622b` 基準）:
> - `engine-wasm/src/lib.rs` の `pub fn evaluate_terminal(request_json: &str) -> String`: `{initial_sfen, plies:[{s,g}]}` を parse → `Kifu` を再構築（`sfen_to_position`＋各 ply の `Action::from_usi` を push）→ **`match engine::terminate::evaluate(&kifu) { Ongoing → {"status":"ongoing"}; …8 分岐のインラインマッピング… → (kind,outcome) }`** → `{"status":"terminal","kind":..,"outcome":..}` を format。末尾で `use engine::archive::{Outcome, ResultKind}; use engine::terminate::{DrawKind, LossKind, Terminal}; use engine::types::Side;` をローカル import している。
> - `engine::terminate::terminal_to_result(&Terminal) -> Option<(ResultKind, Outcome)>`（Step A・Ongoing は None・現行インラインと一字一句一致）。
> - `web/board.js`: `evaluate_terminal as wasmEvaluateTerminal`（6 行）、`evaluateTerminalAt`（204 行で `wasmEvaluateTerminal` を呼ぶ）。**この段では board.js を変更しない**（返す JSON 形が同一なので）。
> 関連文書: `不完全将棋_終局判定の単一正本化アーク_概観と段組`、Step A・B 指示書。
> 性格: Step C（C-minimal）は**「engine-wasm の `evaluate_terminal` を、engine の `terminal_to_result` を呼ぶ形へ直し、Step A が生んだ盤面マッピングの重複を消す」**。返す JSON（`{"status":"ongoing"}` / `{"status":"terminal","kind":..,"outcome":..}` / エラー）は完全に同一——**web の挙動は不変**。web の投了経路（`resultOverride`）は畳まない（正準本文＝記録係アークの領分）。engine-wasm のみ・wasm 再ビルド・`?v=` 前進・配布据え置き。

---

## 0. 目的と範囲

- **作るもの**:
  1. `engine-wasm/src/lib.rs` の `evaluate_terminal` を書き換え：インラインの 8 分岐マッピングを `terminal_to_result(&evaluate(&kifu))` へ置換。不要になったローカル import を削除。
  2. wasm 再ビルドと web への配置、web `?v=` 前進。
- **位置づけ**: 終局判定の単一正本化アークの **Step C（C-minimal）**。盤面終局のマッピングを web も TUI も engine の単一正本（`terminal_to_result`）で共有する状態にする（最長手数バグの源＝盤面マッピングの重複が消える）。
- **作らないもの（＝理由つき）**:
  - **web の `currentResult` 二経路の統合（C-full）**: 投了を plies に統一するとアーカイブ正準本文が変わる。それは正準本文を Web/TUI 双方で設計する記録係アーク（先送り済み）の領分。今そこへ先食いしない（C-minimal・作り手判断）。したがって `board.js` は**無変更**（`resultOverride`＋`evaluateTerminalAt` のまま）。
  - **engine-wasm に `game_result` を公開**: web は盤面経路（`evaluate_terminal`）のまま。`game_result` は TUI（Step D）と 4b が使う。engine-wasm には要らない。
  - **engine・protocol・tui の変更**: この段は engine-wasm のみ。
  - **`evaluate` の投了対応**: web は投了組手を plies に入れないので `evaluate_terminal` に投了は来ない（現行どおり）。防御は不要。

---

## 1. `evaluate_terminal` の書き換え

kifu 再構築（`initial_sfen` の parse・各 ply の `Action::from_usi` を push・エラー処理）は**そのまま**。終局評価の部分だけを差し替える。

**置換前**（末尾のローカル import ＋ 8 分岐マッチ ＋ format）:

```rust
    use engine::archive::{Outcome, ResultKind};
    use engine::terminate::{DrawKind, LossKind, Terminal};
    use engine::types::Side;

    let (kind, outcome) = match engine::terminate::evaluate(&kifu) {
        Terminal::Ongoing => return r#"{"status":"ongoing"}"#.to_string(),
        Terminal::Loss { loser: Side::Sente, kind: LossKind::Mate } => (ResultKind::Mate, Outcome::GoteWins),
        // … 残り 7 分岐 …
    };

    format!(
        r#"{{"status":"terminal","kind":"{}","outcome":"{}"}}"#,
        kind.to_str(),
        outcome.to_str()
    )
```

**置換後**:

```rust
    match engine::terminate::terminal_to_result(&engine::terminate::evaluate(&kifu)) {
        None => r#"{"status":"ongoing"}"#.to_string(),
        Some((kind, outcome)) => format!(
            r#"{{"status":"terminal","kind":"{}","outcome":"{}"}}"#,
            kind.to_str(),
            outcome.to_str()
        ),
    }
```

- **不要 import を削除**: `use engine::archive::{Outcome, ResultKind};`・`use engine::terminate::{DrawKind, LossKind, Terminal};`・`use engine::types::Side;` はこの関数末尾ではもう不要（`kind`/`outcome` は返り値のもので、`.to_str()` はメソッド呼び出しなので型 import 不要）。ただし関数の**他の場所**（kifu 再構築で `engine::types::Action`/`Ply` 等）で使っている import は残す。削除は「終局評価ブロックのためだけにあったローカル import」に限る。`cargo clippy` の未使用 import 警告で取りこぼしを検出できる。
- **返る JSON は同一**: `Ongoing`→`{"status":"ongoing"}`、終局→`{"status":"terminal","kind":..,"outcome":..}`、parse エラー→従来どおり（この段は触らない）。`terminal_to_result` が現行 8 分岐と一致するので、web が受け取るバイト列は変わらない。

**注意（挙動保存の前提）**: `evaluate_terminal` は投了組手を含まない plies を前提に呼ばれる（web は投了を `resultOverride` で扱い plies に入れない）。この前提は現行と同じ——`evaluate` は投了組手で panic するが、web はそれを渡さない。C-minimal はこの前提を変えない。

## 2. ビルド・配置・版

- wasm を再ビルドし、成果物を `web/` の配置先へ（現行の手順に従う）。
- web の `?v=` を前進（キャッシュ更新）。**配布版は据え置き**（web のみ・挙動不変）。
- `cargo build`（wasm target 含む）・`cargo clippy -D warnings` 通過。engine・protocol・tui・board.js に差分なし。

## 3. テスト・受け入れ

- **挙動不変の確認（web）**: 盤面終局の各種（詰み・玉取り・相討ち・千日手・最長手数）で、web の終局表示・アーカイブ結果が**従来と同一**であること。`evaluateTerminalAt` が返す JSON が変わっていないこと（`{"status":"terminal","kind":..,"outcome":..}` / `{"status":"ongoing"}`）。
- **投了経路の不変**: 投了時の `resultOverride` 経路は無変更なので、投了の表示・結果が従来どおり。
- **受け入れ条件**:
  - `evaluate_terminal` が `terminal_to_result` 経由になり、engine-wasm 内の盤面マッピングの重複が消えている。
  - web が受け取る JSON がバイト単位で同一（web 挙動不変）。
  - engine・protocol・tui・board.js に差分ゼロ。`?v=` 前進・配布据え置き。

## 末尾要約

engine-wasm の `evaluate_terminal` を、engine の `terminal_to_result(&evaluate(&kifu))` を呼ぶ形へ書き換え、Step A が engine へ移した盤面マッピングの重複（engine-wasm 内のインライン 8 分岐）を消す。kifu 再構築と返す JSON は完全に同一で、web の挙動はバイト単位で不変。web の `currentResult` 二経路（`resultOverride`＋`evaluate_terminal`）は畳まない——投了を plies に統一するとアーカイブ正準本文が変わり、それは記録係アーク（Web/TUI 双方で正準本文を設計する場）の領分だから。engine-wasm のみの変更・wasm 再ビルド・web `?v=` 前進・配布据え置き。これで盤面終局は web も TUI も engine の単一正本を共有する。

## 不変の原則

- **盤面マッピングは単一正本**: `terminal_to_result`（engine）を web（engine-wasm）も TUI（Step D の game_result 経由）も共有。重複を残さない。
- **挙動保存**: web が受け取る JSON はバイト単位で同一。board.js は無変更。
- **記録係の領分に先食いしない**: web の投了を plies に統一する統合（C-full）はしない。正準本文は記録係アークで Web/TUI 双方を見据えて設計する。
- **engine-wasm のみ**: engine・protocol・tui は触らない。この段は重複解消の一点。
- **過ぎたるは及ばざる**: engine-wasm に `game_result` を公開しない（web は盤面経路のまま）。投了防御も入れない（web は投了組手を渡さない）。
