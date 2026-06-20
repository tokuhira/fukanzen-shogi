# Fukanzen Shogi / 不完全将棋

**A simultaneous-move variant of shogi where what is hidden is not the board, but the next move.**

---

## English

### What Is Fukanzen Shogi?

Fukanzen Shogi ("Incomplete Shogi") is a variant of traditional Japanese shogi built on a single, radical premise: **the board is fully visible to both players at all times, but each player's next move is secret until both have committed simultaneously.**

Every turn, both players choose their move in secret and reveal them at the same instant. The hidden element is not state (the positions of all pieces and captured holdings are always public) but *action* — and the moment of revelation, the board becomes fully public again.

This design rests on three structural goals:

1. **Preserving material intuition** — The piece values and exchange logic cultivated by centuries of traditional shogi carry over as directly as possible.
2. **Eliminating first-move advantage** — By abolishing turns entirely, the structural asymmetry between Sente (the traditional "first player") and Gote disappears. Both players move on every turn; there is no passing, no alternation.
3. **Rewarding aggressive play** — Passive repetition is disincentivized in favor of moves that take positional risk in pursuit of material gain.

### The Core Mechanics

**Simultaneous resolution** proceeds in three stages each turn: (1) both players privately commit to a legal move, (2) all pieces jump simultaneously to their destinations — there is no continuous travel, no intermediate state — and (3) conflicts at final squares are resolved.

**Collision resolution** is designed so that it changes only the *certainty* of an outcome, never its *expected value*:

- **Capture (4.1):** If a piece moves to a square where an opponent's piece remains (did not move away), it captures that piece — regardless of the relative value of attacker and defender. A pawn can capture a rook.
- **Escape (4.2):** If the target piece moved away simultaneously, it is not captured; the attacker occupies the vacated square.
- **Clash on the same square (4.3):** If both pieces move to the same square, both are removed and each becomes the other player's captured holding — a mutual exchange.
- **Swap clash (4.4):** If two pieces exchange positions (each moves to the other's origin), they clash identically, even though a naive destination-only check would see no collision.
- **Sengoku Musou exception (4.7):** If one of the two colliding pieces is a king, mutual destruction does not apply. The king unilaterally captures the enemy piece, and the king occupies the collision square. This exception covers both swap clashes (4.4) and same-square clashes (4.3). The same-square case arises when the king retreats to a safe empty square while the opponent simultaneously drops a piece there; the king wins and claims the dropped piece. This exception only fires when the king's destination was safe at commitment time (enforced by the existing king-safety rule in `legal_actions`), so it exclusively counters *unguarded* pieces.
- **Path non-interference (4.6):** Sliding pieces (rook, bishop, lance, dragon, horse) pass through any square that an opponent piece simultaneously lands on mid-path. Only the final square is adjudicated.

**King safety** becomes probabilistic. A king may not move to a square currently attacked by the opponent (the traditional prohibition is preserved at the moment of commitment). However, since the opponent also moves simultaneously, a square that was safe at commitment time may be occupied or attacked by move-end. A king's escape to a legal square is guaranteed safe for that turn; but declining to escape — responding with a capture or interposition instead — is a gamble whose outcome depends on what the opponent actually played.

**Termination** occurs in three ways: (5.1) *Definite mate* — a player has no legal moves at commitment time, which is equivalent to traditional checkmate; (5.2) *King's death* — a king is captured as the result of simultaneous resolution, which can occur even without a prior check; or (5.3) *Resignation*. If both conditions arise simultaneously, the result is a draw.

> **Open questions (spec §7):** Repetition (sennichite), continuous check, the precise reading of the pawn-drop checkmate prohibition under simultaneous commitment, and the notation for entering-king declarations are all listed as undecided in the current specification. They are *not* resolved by this implementation; placeholder behavior is marked with code comments.

### This Repository — Phases 1 & 2

**Phase 1 — Rule engine + verification CLI** delivers:

- **A pure Rust rule engine** (`engine/`) — a library crate with zero I/O, no `async`, no RNG, no networking. Its public API is a set of pure functions: `legal_actions`, `resolve`, `check_status`, and serialization helpers.
- **A verification CLI** (`cli/`) — a single-process tool where one person inputs moves for both sides in USI notation, used to manually verify that the engine behaves as specified. Kept as a machine-readable pipe interface; not modified by later phases.
- **A regression test suite** — 33 tests covering all concrete examples from the implementation spec: collision cases (capture, escape, same-square clash, swap, path pass-through, drop clash, promoted-piece reversion, Sengoku Musou swap and drop-clash), move generation (king safety, check evasion, nifu, uchi-fu-dzume, backed-piece prevention), termination (definite mate, king's death, simultaneous king capture, draw conditions, counter-play backfire, rook pass-through), serialization round-trips, and canonical initial-position verification against the spec's authoritative SFEN.

USI notation is used throughout for moves (`7g7f`, `P*5e`, `2b3a+`). Position serialization follows SFEN with a fixed sentinel (`b`) in place of the turn field, which does not exist in this variant; the canonical initial position is defined by a single `INITIAL_SFEN` constant rather than hardcoded piece placement, making the spec document the single source of truth. Canonical serialization — deterministic board + holdings + move-number bytes — is ready for a SHA-256 layer to be added in a later phase. A separate content serialization (board + holdings only, no move number) is used for repetition detection.

**Phase 2 — TUI verification desk** adds:

- **A full-screen TUI** (`tui/`) — built on [ratatui](https://ratatui.rs) + crossterm. One person operates both sides with cursor keys and mouse on a single screen, with no secrecy — pure verification mode.
- **Interactive legal-move highlighting** — selecting any piece (on board or in hand) immediately highlights every legal destination for that piece. This makes engine correctness visually inspectable: king-safety exclusions, check-evasion filtering, Sengoku Musou non-application, and piece-drop constraints all appear as the presence or absence of yellow highlights.
- **Simultaneous resolution UI** — build Sente's move, then Gote's move, then press Enter to resolve both atomically. The resolution result (captures, escapes, clashes, Sengoku Musou activations, king deaths) is displayed as text in the info panel.
- The engine crate is untouched; the TUI is strictly a new shell on top of the existing public API.

### Roadmap (future phases, not yet implemented)

- **Phase 3:** Wasm compilation via `wasm-bindgen`; browser-based board UI; canonical hash exchange between clients for mutual board verification; commitment-reveal protocol for genuinely secret simultaneous commitment; disconnect recovery via move-history checkpointing.
- **Phase 4:** CPU opponent (search + evaluation).
- **Phase 5+:** Smartphone app; multi-language engine re-implementations with cross-diffing against the Rust reference.

The engine is designed from the start so that all of the above are *shells* around an unchanged core. The engine itself will never gain I/O, networking, or randomness.

---

## 日本語

### 不完全将棋とは

不完全将棋は日本将棋を基底とした変種であり、その核心は一つの問いに集約される。**「隠されるのは盤面ではなく、次の一手である」。**

盤上の駒の位置・種類・持ち駒はすべて両者に公開される。しかし各ターン、両者は互いの着手を知らないまま自分の着手を確定し、同時に開示する。隠されるのは状態ではなく行動（着手）であり、開示の瞬間に盤面は再び完全公開へ戻る。

この設計を貫く三つの方針は次のとおりである。

1. **損得勘定の維持** — 伝統的な将棋が培った駒の損得感覚を可能な限り保つ。
2. **先後の公平性** — 手番を廃し両者同時着手とすることで、先手・後手の非対称を構造的に排除する。パスも手待ちも存在しない。
3. **膠着打開への報酬** — 消極的な反復より、リスクを取って動く積極策が報われる。

### ゲームの核心

**同時着手の三段階解決:** 各ターンは「両者の秘密裏の着手確定 → 全駒の同時移動（瞬間移動であり途中経過は存在しない） → 最終マスでの衝突解決」の順に進む。

**衝突解決の設計原則は「損得を変えず確実性だけを変える」ことである。**

- **取得（4.1）:** 移動先に相手の駒が留まっていれば、取りに行った駒の価値の高低に関わらず取得が成立する。歩が飛を取れる。
- **逃げた駒（4.2）:** 移動先にいた相手の駒が同時に別マスへ移動していた場合、取得は発生しない。
- **同一マスへの相討ち（4.3）:** 両駒が同一マスへ到達した場合、相討ちとなり双方向に持ち駒となる（交換）。
- **スワップの相討ち（4.4）:** 互いに相手の旧位置を移動先とする正面衝突も相討ちとなる。「逃げずにぶつかれば刺し違える」。
- **戦国無双特則（4.7）:** 衝突の当事者の一方が玉の場合、相討ちを適用しない。玉は相手駒を一方的に取得して衝突マスを占める。スワップ（4.4）と同一マスへの相討ち（4.3）の両方に適用される。同一マス相討ちへの拡張は「玉が安全な空きマスへ退避しても、相手が持ち駒を打ち込めば以前は死んでいた」問題を解消する。着手確定時点での玉の侵入禁止（合法手生成の既存ロジック）により、この特則は後ろ盾のない駒に対してのみ発動する。
- **経路の非干渉（4.6）:** 走り駒の経路途中に相手の駒が着地しても干渉しない。判定は最終マスでのみ行う。

**玉の安全性は確率的になる。** 玉は、着手開始時点で相手の利きのあるマスへは移動できない（伝統的ルールをそのまま引き継ぐ）。ただしこれは着手確定時点の判定であり、相手も同時に動くため、安全だったマスが移動後に危険になりうる。安全なマスへの玉の逃げはそのターン必ず助かる。しかし合駒や反撃で応じることは賭けであり、相手が実際に玉へ向かっていれば取られる。

**終了条件は三種:** 確定的詰み（着手不能、伝統的詰みと一致）・玉の死（衝突解決の結果として玉が取られる）・投了。両者が同時に成立した場合は引き分け。

> **未確定事項（仕様書 §7）:** 千日手の成立時の扱い、連続王手の千日手の読み替え、打ち歩詰めの厳密な再形式化、入玉宣言法の読み替えはいずれも未確定です。本実装ではこれらを勝手に確定させず、暫定処理とコードコメントによる印として引き継いでいます。

### このリポジトリ — 第一段階・第二段階

**第一段階 — ルールエンジン＋検証用 CLI** の成果物:

- **Rust ルールエンジン**（`engine/`）— I/O・非同期・乱数・ネットワーク依存を一切持たない純粋なライブラリクレート。公開 API は `legal_actions`・`resolve`・`check_status` と直列化関数群からなる純粋関数群。
- **検証用 CLI**（`cli/`）— 一人が両陣営の着手を USI 記法で入力し、一局を最後まで進められる検証モード。秘匿性なし・単一プロセス。機械可読の口として以後の段階でも無改変で温存する。
- **回帰テスト群** — 仕様書の具体例を写した 33 本のテスト（衝突解決・戦国無双特則（スワップ＋同一マス打ち込み）・合法手生成・後ろ盾検証・終了判定・取り合いの裏目・合駒貫き・両玉同時取得・直列化・初期局面の正本 SFEN 照合）がすべて通過している。

着手記法は USI 準拠（例: `7g7f`、`P*5e`、`2b3a+`）。局面の SFEN 手番フィールドは固定値 `b`（不完全将棋に手番は存在しない）。初期局面は正本 SFEN 定数 `INITIAL_SFEN` をパースして生成し、仕様書が唯一の出典となる。正準直列化（盤面＋持ち駒＋手数）は第三段階でのハッシュ計算への前方互換として、千日手検出用の内容直列化（手数除く）と区別して設計済み。

**第二段階 — TUI 検証卓** の成果物:

- **全画面 TUI**（`tui/`）— [ratatui](https://ratatui.rs)（ターミナル UI ライブラリ）と crossterm による全画面対話インターフェース。一人が先手・後手の両着手をカーソルとマウスで組み立て、同時解決する検証モード（秘匿なし・単一プロセス・単一画面）。
- **合法手インタラクティブ提示**（本段階の目玉機能）— 盤上・駒台の駒を選ぶと、その駒の合法な移動先・打ち先が即座に黄色ハイライトされる。玉の侵入禁止・王手回避による絞り込み・戦国無双の非発動・打ち駒の制約などがハイライトの有無として目視確認できる。
- **同時解決 UI** — 先手の着手を組み、後手の着手を組み、Enter で両着手を同時に `resolve` する三拍子の操作モデル。解決結果（取得・逃げ・相討ち・戦国無双発動・玉の死）をテキストでパネルに表示する。
- エンジンクレートは無改変。TUI は既存の公開 API のみを叩く新たな殻として追加した。

### 今後の計画（未実装）

- **第三段階:** Wasm 化・ブラウザ UI・コミットメント方式（commit-reveal）による秘匿同時着手・盤面ハッシュ相互検証・中断救済。
- **第四段階:** CPU 対戦（探索・評価関数）。
- **第五段階以降:** スマートフォンアプリ・多言語実装と差分テスト。

エンジンは「共通の核と交換可能な殻」の設計原則に基づき、これらはすべてエンジンの外側に積む予定である。エンジン本体にはいかなる I/O も追加しない。

---

## Detailed Specification / 詳細仕様

- [不完全将棋 ルール仕様 v0.4](docs/不完全将棋_ルール仕様_v0.4.md) — 現行仕様。戦国無双特則（§4.7）をスワップ（4.4）と同一マス相討ち（4.3）の両方に拡張
- [不完全将棋 ルール仕様 v0.3](docs/不完全将棋_ルール仕様_v0.3.md) — 戦国無双特則（§4.7）をスワップ限定で追加
- [不完全将棋 ルール仕様 v0.2](docs/不完全将棋_ルール仕様_v0.2.md) — 初期局面の正本 SFEN と SFEN 手番フィールドの確定的な扱い（`b` 固定）を追記
- [不完全将棋 ルール仕様 v0.1](docs/不完全将棋_ルール仕様_v0.1.md) — 初版ルール定義
- [不完全将棋 実装指示書 — 第一段階](docs/不完全将棋_実装指示書_第一段階.md) — Phase 1 の設計・実装指針（仕様書 v0.4 対応）
- [不完全将棋 実装指示書 — 第二段階 TUI 検証卓](docs/不完全将棋_実装指示書_第二段階_TUI検証卓.md) — Phase 2 の設計・実装指針（ratatui による TUI 検証卓）

---

## Build & Run

**Requirements:** Rust stable (edition 2021+), Cargo.

```sh
# Build all crates
cargo build

# Run all tests (engine regression suite — 33 tests)
cargo test

# Run the verification CLI (text I/O, machine-readable)
cargo run --bin fukanzen-shogi

# Run the TUI verification desk (full-screen, interactive)
cargo run --bin fukanzen-shogi-tui
```

### TUI key bindings

| Key | Action |
|-----|--------|
| `↑↓←→` | Move cursor on board |
| `Enter` / `Space` | Select piece or confirm destination |
| `d` / `Tab` | Toggle hand-piece selection mode (then `←→` to cycle) |
| `1`–`7` | Directly select hand piece (歩香桂銀金角飛) |
| `y` / `p` | Promote (in promotion dialog) |
| `n` | No promotion |
| `Esc` | Cancel selection / dismiss dialog |
| `Enter` (ResolveReady) | Resolve both moves simultaneously |
| `u` | Undo (or reset current turn's input) |
| `r` | Resign current side |
| `s` / `S` | Save game record (default path / prompt for path) |
| `l` / `L` | Load game record (default path / prompt for path) |
| `f` | Display current position as SFEN |
| `m` | List all legal moves for current side |
| `?` | Toggle help overlay |
| `q` | Quit |

Mouse clicks on board squares are supported; clicking a hand piece area toggles hand-piece selection mode.

### CLI usage

```
先手 の着手を入力 (USI例 7g7f, P*5e): 7g7f
後手 の着手を入力 (USI例 7g7f, P*5e): 3c3d

Display commands:
  :board                — redisplay the board
  :kifu                 — display the full move list so far
  :moves [s|g]          — list legal moves (default: current side); s=Sente, g=Gote
  :sfen                 — display current position in SFEN notation

Game commands:
  :quit                 — exit
  :resign [s|g]         — resign (default: current side); s=Sente, g=Gote
  :undo                 — take back the last ply

File commands:
  :load <path>          — load game record from file
  :save <path>          — save game record to file
```

Moves are entered in USI notation: `7g7f` (move from 7g to 7f), `P*5e` (drop a pawn at 5e), `2b3a+` (move with promotion).

---

## License

[MIT](LICENSE) © 2026 tokuhira
