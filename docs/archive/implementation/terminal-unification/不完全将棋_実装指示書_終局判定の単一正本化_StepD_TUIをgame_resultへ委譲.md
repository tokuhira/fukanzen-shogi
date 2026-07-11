# 不完全将棋 実装指示書 — 終局判定の単一正本化 Step D：TUI の終局判定を `game_result` へ委譲（最長手数を塞ぐ）

> 対象実行者: Claude Code（Sonnet 5）
> 前提: Step C 着地（HEAD `64aa7ca`。engine `terminal_to_result`・protocol `game_result`・engine-wasm は共有マッピング経由）。**この段がアークの本丸**——TUI の手組みの終局判定（`resolve_turn` の玉の死→千日手→着手不能）と online の turn-action 投了を `protocol::game_result` へ委譲し、**最長手数500組手の穴を構造的に塞ぐ**（＋投了の勝敗を単一正本へ）。`GameOverKind` は TUI の表示語彙として残し、`(ResultKind,Outcome)→GameOverKind` の写像を一つ置く。**local の即時投了（`app.rs::resign()`）は直接設定のまま**——それは組手にならない宣言（盤面が完成する前の概念的に別の行為）。締めで配布パッチ bump（v0.12.2）——500組手バグ修正が乗る。ビルド・LAN/クラウド検証は Sonnet 側。
> 関連する現物（すべて実地で確認済み・HEAD `64aa7ca` 基準）:
> - `tui/Cargo.toml`: protocol に依存（online.rs が `protocol::ClientSession` を使用）＝`protocol::game_result` を呼べる。
> - `tui/src/app.rs`:
>   - `resolve_turn`: `resolve(&pos, sente, gote)` で `last_resolution`（この手の盤面描写）→ `kifu.push(Ply)` → **手組み判定**（`check_king_death(res.event)`→`check_sennichite(kifu)`→`check_status(next_pos)`、各分岐で `last_resolution.push("→ …")` ＋ `Phase::GameOver(GameOverKind::…)`）→ 続行時 `Phase::SenteInput`。**`evaluate`/`game_result` を使わず・`MaxTurns` を判定しない**（バグ）。
>   - `pub enum GameOverKind { SenteWins(WinReason), GoteWins(WinReason), Draw(DrawReason) }`、`WinReason{Resign,KingDied,Checkmate}`、`DrawReason{BothKingDied,BothCheckmate,Sennichite,MutualResign}`（**MaxTurns 無し**）。
>   - `pub fn game_over_text(kind: &GameOverKind) -> &'static str`（793-803）: 各 variant → 日本語。ui.rs（`render_game_over_popup`・status 表示・2 箇所）が使用。例: `GoteWins(KingDied)`→"後手の勝ち（先手玉が取られた）"、`Draw(Sennichite)`→"引き分け（千日手）"。
>   - `pub fn resign(&mut self)`（604-）: **local 即時投了**。`Phase::SenteInput` なら `GameOver(GoteWins(Resign))`、`GoteInput` なら `GameOver(SenteWins(Resign))`。組手を push しない（宣言）。
> - `tui/src/online.rs::resolve_completed_turn`: turn-action 投了を `is_resign` で先取りし `GameOverKind::{Draw(MutualResign)/GoteWins(Resign)/SenteWins(Resign)}` を直接設定（`resolve` を通さない）。非投了は `app.resolve_turn()` ＋ `kifu.push(Ply)`（**app.kifu と param kifu の二重管理**が既存）。
> - `protocol::game_result(kifu: &Kifu) -> Option<(ResultKind, Outcome)>`（Step B）: 最後の組手の投了を先に判定（先手投了→GoteWins 等）、でなければ `terminal_to_result(evaluate(kifu))`。`None` は未了。
> - `engine::archive::{ResultKind{Mate,KingDeath,SwapDraw,Sennichite,MaxTurns,Resign,Unfinished,Other}, Outcome{SenteWins,GoteWins,Draw,None}}`。
> 関連文書: `不完全将棋_終局判定の単一正本化アーク_概観と段組`、Step A/B/C 指示書、`design/不完全将棋_ルール仕様_v0.6`（§5.7 最長手数500組手・§5.4 両者投了）。
> 性格: Step D は**「TUI の盤面終局判定（`resolve_turn`）と online の turn-action 投了を `protocol::game_result` へ委譲し、最長手数を塞ぎ、投了の勝敗を単一正本へ寄せる」**。`GameOverKind` は表示語彙として残す（`game_over_text` は UI の関心事）。検出は `game_result` に一本化、表示は TUI 所有——検出と表示の綺麗な分離。local 即時投了は直接設定のまま。挙動保存：既存の終局（詰み=着手不能・玉取り・相討ち・千日手・投了）の判定結果と表示文言を保ち、**新たに最長手数を終局させる**。触るのは app.rs（DrawReason 追加・写像・resolve_turn）と online.rs（resolve_completed_turn 投了枝）のみ。

---

## 0. 目的と範囲

- **作るもの**:
  1. `app.rs`: `DrawReason::MaxTurns` を追加＋`game_over_text` に対応（§1）。
  2. `app.rs`: `game_over_from_result(ResultKind, Outcome) -> GameOverKind` の写像（§2）。
  3. `app.rs::resolve_turn`: 手組み判定を `game_result` へ置換（§3）。**最長手数を塞ぐ本丸**。
  4. `online.rs::resolve_completed_turn`: turn-action 投了枝を `game_result` へ委譲（§4）。
- **位置づけ**: 終局判定の単一正本化アークの**本丸（Step D）**。TUI が単一正本に乗り、最長手数バグが構造的に消える。その後 4b は `game_result` を直接呼ぶ。
- **作らないもの（＝理由つき）**:
  - **`app.rs::resign()`（local 即時投了）の変更**: 組手にならない宣言（盤面完成前）。kifu 由来の終局でないので `game_result` を通さず直接設定のまま。ただし勝敗は現状どおり（`GoteWins/SenteWins(Resign)`）で `game_result` と一致。
  - **`GameOverKind` の廃止（(ResultKind,Outcome) への全面置換）**: 表示語彙 `game_over_text` と ui.rs を大きく触ることになる。検出の単一正本化はこの段の目的で足り、表示語彙は TUI 所有のまま写像一つで橋渡しする（過ぎたるは及ばざる）。
  - **app.kifu / param kifu の二重管理の解消**: 既存の構造。この段では触らない（別畝）。
  - **engine・protocol・engine-wasm・web の変更**: 済み（A/B/C）またはこの段の対象外。
  - **4b（観戦配信）**: 別段。ただし Step D 着地後、4b の `spectate_result` は `game_result` を直接呼べばよい（私が綻びさせた対応表は不要）。

---

## 1. `DrawReason::MaxTurns` の追加と表示

```rust
pub enum DrawReason {
    BothKingDied,
    BothCheckmate,
    Sennichite,
    MaxTurns,       // 追加（ルール v0.6 §5.7）
    MutualResign,
}
```

`game_over_text` に一行:

```rust
        GameOverKind::Draw(DrawReason::MaxTurns) => "引き分け（最長手数）",
```

（文言は web の result-view と揃える。web が「最長手数（500組手）」等なら合わせる。表示語彙は TUI/web で人が読む文字列なので、齟齬なく。）

## 2. `game_over_from_result`（`(ResultKind, Outcome)` → `GameOverKind`）

`game_result` の出力（アーカイブ語彙）を TUI の表示語彙へ写す。**全単射**（`_ =>` を使わず網羅）。

```rust
use engine::archive::{Outcome, ResultKind};

pub fn game_over_from_result(kind: ResultKind, outcome: Outcome) -> GameOverKind {
    use GameOverKind::*;
    match (kind, outcome) {
        (ResultKind::Mate, Outcome::SenteWins)      => SenteWins(WinReason::Checkmate),
        (ResultKind::Mate, Outcome::GoteWins)       => GoteWins(WinReason::Checkmate),
        (ResultKind::Mate, Outcome::Draw)           => Draw(DrawReason::BothCheckmate), // MutualMate
        (ResultKind::KingDeath, Outcome::SenteWins) => SenteWins(WinReason::KingDied),
        (ResultKind::KingDeath, Outcome::GoteWins)  => GoteWins(WinReason::KingDied),
        (ResultKind::SwapDraw, Outcome::Draw)       => Draw(DrawReason::BothKingDied),
        (ResultKind::Sennichite, Outcome::Draw)     => Draw(DrawReason::Sennichite),
        (ResultKind::MaxTurns, Outcome::Draw)       => Draw(DrawReason::MaxTurns),
        (ResultKind::Resign, Outcome::SenteWins)    => SenteWins(WinReason::Resign),
        (ResultKind::Resign, Outcome::GoteWins)     => GoteWins(WinReason::Resign),
        (ResultKind::Resign, Outcome::Draw)         => Draw(DrawReason::MutualResign),
        // game_result が返さない組み合わせ（Unfinished/Other/None 等）は終局として来ない。
        // 網羅のため明示的にし、来たら panic ではなく妥当な既定へ倒すか、
        // unreachable! で「game_result の契約違反」を早期に検出する。実装者判断で
        // 「game_result の全出力を尽くしているか」をテストで固定する（§5）。
        other => unreachable!("game_result が想定外の結果を返した: {:?}", other),
    }
}
```

- `game_result` が実際に返す 11 通り（Mate×3・KingDeath×2・SwapDraw・Sennichite・MaxTurns・Resign×3）を尽くす。それ以外は `game_result` の契約違反なので `unreachable!`。§5 のテストで全出力を網羅確認する。

## 3. `resolve_turn` を `game_result` へ（本丸）

`resolve()` による `last_resolution`（この手の盤面描写）は**残す**（narration）。その後の**手組み判定（玉の死→千日手→着手不能）を `game_result` 一本へ置換**する。

```rust
    pub fn resolve_turn(&mut self) {
        let sente = match self.sente_action { Some(a) => a, None => return };
        let gote  = match self.gote_action  { Some(a) => a, None => return };

        let pos = self.current_pos();
        let res = resolve(&pos, sente, gote);                 // narration 用（投了は来ない）
        self.last_resolution = build_resolution_text(&pos, sente, gote, &res.event);

        self.kifu.push(Ply { sente, gote });
        self.sente_action = None;
        self.gote_action = None;
        self.show_all_moves = false;

        // 終局判定は単一正本 game_result へ（玉の死・詰み・千日手・最長手数を一元判定）。
        if let Some((kind, outcome)) = protocol::game_result(&self.kifu) {
            let go = game_over_from_result(kind, outcome);
            self.last_resolution.push(format!("→ {}", game_over_text(&go)));
            self.phase = Phase::GameOver(go);
            return;
        }

        // 続行
        self.phase = Phase::SenteInput;
        self.cursor_file = 5;
        self.cursor_rank = 9;
        self.message = String::new();
    }
```

- **削除**: `check_king_death`/`check_sennichite`/`check_status` の三ブロックと、各分岐の `last_resolution.push("→ …")`・`GameOverKind` 直接構築。これらは `game_result` ＋ `"→ " + game_over_text` に集約される。
- **`"→ " + game_over_text` の一致**: 現行の各文言（"→ 後手の勝ち（先手玉が取られた）" 等）は `game_over_text(&go)` に "→ " を付けたものと一致する（現物で確認済み・玉の死/着手不能/千日手すべて）。よって表示は保存され、**最長手数だけ新たに "→ 引き分け（最長手数）" が出る**。
- **resolve() は残す**: narration（この手で何が動いた/取られた）は `res.event` から作る。`resolve_turn` は投了組手で呼ばれない（online は §4 で先取り・local 即時投了は §0 で別扱い）ので `resolve()` は panic しない。
- 使用するもの: `use engine::archive::{ResultKind, Outcome};`（`game_over_from_result` 用）。`check_king_death`/`check_sennichite`/`check_status`/`GameEnd`/`GameStatus` の import はこの関数で不要になれば削除（他で使っていれば残す。clippy が拾う）。

## 4. `online.rs::resolve_completed_turn` の投了枝を `game_result` へ

turn-action 投了の勝敗を、手組みでなく `game_result` に委ねる。投了組手を kifu に積んで `game_result` を呼ぶ。

```rust
    let s_resign = sente_action.is_resign();
    let g_resign = gote_action.is_resign();
    if s_resign || g_resign {
        // 投了組手を online の kifu へ積み、game_result に勝敗を委ねる（単一正本）。
        kifu.push(Ply { sente: sente_action, gote: gote_action });
        if let Some((kind, outcome)) = protocol::game_result(kifu) {
            app.phase = Phase::GameOver(app::game_over_from_result(kind, outcome));
        }
        return;
    }

    // 通常の着手（従来どおり）: app.resolve_turn() ＋ param kifu.push
    app.sente_action = Some(sente_action);
    app.gote_action = Some(gote_action);
    app.resolve_turn();                              // ← §3 で game_result 経由に
    kifu.push(Ply { sente: sente_action, gote: gote_action });
    // …（非終局なら次ターンへ・side==Gote の上書き等、従来どおり）…
```

- **削除**: `use crate::app::{DrawReason, GameOverKind, WinReason};` と `match (s_resign,g_resign)` の手組み。勝敗は `game_result`＋`game_over_from_result` へ。
- **二重管理の扱い**: 投了枝では param `kifu` に積んで `game_result(kifu)` を呼ぶ（app.kifu には積まない＝従来も積んでいない・投了は盤面手でないので表示に影響なし）。非投了枝は従来どおり（app.resolve_turn が app.kifu で判定・param kifu にも push）。両 kifu は組手が並行するので結果は一致（対局はここで終わるので以降の齟齬もなし）。
- `app::game_over_from_result` は §2 で app.rs に pub で置く。

## 5. テスト・受け入れ・版

- **単体（app.rs）**: `game_over_from_result` が `game_result` の全出力 11 通りを網羅し、期待 `GameOverKind` を返す（`unreachable!` に落ちない）。`DrawReason::MaxTurns`→`game_over_text` が文言を返す。
- **LAN/クラウド/ローカルの手検証**:
  1. **最長手数（本丸）**: 500 組手に達する対局で、**両クライアントが同時に引き分け終局**する（従来 TUI は続行してしまい desync していた）。TUI 単独（local/LAN 両 TUI）でも 500 で引き分け。
  2. **既存終局の保存**: 詰み（着手不能）・玉取り・相討ち（両玉/両着手不能）・千日手・投了（先手/後手/両者）で、判定結果と表示文言（"対局終了: …" と "→ …"）が従来と同一。
  3. **online 投了**: turn-action 投了が `game_result` 経由で正しい勝者に。
  4. **local 即時投了**: `resign()` は従来どおり（変更なし）。
- **受け入れ条件**:
  - `resolve_turn` が `game_result` 経由になり、`check_king_death`/`check_sennichite`/`check_status` の手組みが消えている。TUI が 500 組手で終局する。
  - online turn-action 投了が `game_result` 経由。勝敗が現物・web と一致。
  - 既存終局の表示文言が保存。`cargo build -p fukanzen-shogi-tui`・`cargo clippy -D warnings` 通過。
  - engine・protocol・engine-wasm・web に差分ゼロ。
- **版**: TUI が 500 組手を正しく終局させる**バグ修正**＋投了統一。**配布パッチ bump（v0.12.2）**。`Cargo.toml` を上げ `--version` を揃える。（4b を同じ v0.12.2 に束ねるか別建てかは作り手判断。）

## 末尾要約

TUI の盤面終局判定（`resolve_turn` の玉の死→千日手→着手不能の手組み）と online の turn-action 投了を `protocol::game_result` へ委譲し、**最長手数500組手の穴を構造的に塞ぐ**（＋投了の勝敗を単一正本へ）。`DrawReason::MaxTurns` を追加し、`game_result` のアーカイブ語彙 `(ResultKind,Outcome)` を TUI の表示語彙 `GameOverKind` へ全単射で写す（`game_over_from_result`）。表示 `game_over_text` は TUI 所有のまま——検出は単一正本、表示は UI 所有の綺麗な分離。`resolve()` による narration は残し、`"→ " + game_over_text` で終局文言を保存する。local 即時投了（`resign()`）は組手にならない宣言なので直接設定のまま。既存終局の挙動を保ちつつ最長手数を新たに終局させる。配布パッチ bump（v0.12.2）でアークの本丸を締める。

## 不変の原則

- **検出は単一正本・表示は UI 所有**: 終局検出は `game_result`。`GameOverKind`/`game_over_text` は TUI の表示語彙として残し、写像一つで橋渡し。
- **投了は protocol・即時投了は UI 行為**: turn-action 投了は `game_result`（単一正本）へ。local 即時投了は組手にならない宣言なので直接設定（概念的に別）。
- **narration は残す**: この手の盤面描写（`resolve()`＋`build_resolution_text`）は保つ。終局文言は `"→ " + game_over_text`。
- **挙動保存＋バグ修正**: 既存終局の判定・表示を保ち、最長手数だけ新たに終局させる。全単射は網羅列挙で守る。
- **過ぎたるは及ばざる**: `GameOverKind` を廃止しない（表示語彙は UI）。app.kifu/param kifu の二重管理は触らない。触るのは app.rs と online.rs のみ。
