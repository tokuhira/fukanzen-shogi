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
- **Sengoku Musou exception (4.7):** If exactly one of the two colliding pieces is a king, mutual destruction does not apply. The king unilaterally captures the enemy piece and occupies the collision square. This exception covers both swap clashes (4.4) and same-square clashes (4.3). The same-square case arises when the king retreats to a safe empty square while the opponent simultaneously drops a piece there; the king wins and claims the dropped piece. This exception only fires when the king's destination was safe at commitment time (enforced by the existing king-safety rule in `legal_actions`), so it exclusively counters *unguarded* pieces. **v0.5 addition (§4.7):** When *both* colliding pieces are kings (a king swap), the two Sengoku Musou effects cancel each other out, reverting to a normal mutual clash — both kings are captured and the result is a draw.
- **Path non-interference (4.6):** Sliding pieces (rook, bishop, lance, dragon, horse) pass through any square that an opponent piece simultaneously lands on mid-path. Only the final square is adjudicated.

**King adjacency** is possible in simultaneous-move play: if both kings move toward each other in the same turn, they can legally end up on adjacent squares. This cannot occur in traditional shogi (alternating turns) but is structurally valid here. The resulting endgame duel — each king a single step from the other — introduces a new layer of tactical tension.

**King safety** becomes probabilistic. A king may not move to a square currently attacked by the opponent (the traditional prohibition is preserved at the moment of commitment). However, since the opponent also moves simultaneously, a square that was safe at commitment time may be occupied or attacked by move-end. A king's escape to a legal square is guaranteed safe for that turn; but declining to escape — responding with a capture or interposition instead — is a gamble whose outcome depends on what the opponent actually played.

**Termination** occurs in four ways: (5.1) *Definite mate* — a player has no legal moves at commitment time, which is equivalent to traditional checkmate; (5.2) *King's death* — a king is captured as the result of simultaneous resolution, which can occur even without a prior check; (5.3) *Resignation*; or (5.4) *Draw* — both kings are captured simultaneously, either by two pieces taking each other's king in the same turn or by a king swap (4.7 v0.5). If both players' kings die simultaneously, the result is a draw regardless of cause.

> **Open questions (spec §7):** Repetition (sennichite), continuous check, the precise reading of the pawn-drop checkmate prohibition under simultaneous commitment, and the notation for entering-king declarations are all listed as undecided in the current specification. They are *not* resolved by this implementation; placeholder behavior is marked with code comments.

### Demo

A live web board is available at **[fukanzen-shogi.tokuhira.net](https://fukanzen-shogi.tokuhira.net)**.

Interactive hotsheet mode. One person plays both sides with mouse clicks. Click a piece to see its legal moves as subtle ink dots; build Sente's move, then Gote's, then resolve — the Wasm engine handles rule enforcement, simultaneous resolution, and game-over detection. Navigate the growing kifu with ← / →; returning to any past position and playing from there branches the record. Sumi ink style; no frameworks, no build step.

### Repository Structure

**Rust workspace (core and shells):**

- `engine/` — pure rule engine; zero I/O, no `async`, no RNG, no networking. Public API: `legal_actions`, `resolve`, `check_status`, serialization. The **core**.
- `cli/` — machine-readable verification CLI (text I/O, USI notation). A stable pipe interface kept unmodified across all phases.
- `tui/` — interactive verification desk and **network battle mode** (ratatui + crossterm). TCP shell lives in `tui/src/net.rs`.
- `protocol/` — pure protocol logic (commit-reveal, board-hash verification, ack, identity auth, reconnect recovery, version negotiation). No I/O; fully deterministic tests.

**Web frontend and server (static + serverless):**

- `web/` — static HTML/CSS/JS board; no build step, no frameworks. Interactive hotsheet and browser online battle. Powered by `engine-wasm/` and `protocol-wasm/` (Wasm). Deployed to Cloudflare Pages. See [web/README.md](web/README.md).
- `engine-wasm/` — thin `wasm-bindgen` cdylib exposing `resolve_ply`, `game_status`, `legal_actions`, `build_archive`, and `parse_archive` over a pure SFEN/USI string boundary. Engine core is untouched.
- `protocol-wasm/` — thin `wasm-bindgen` cdylib exposing `ProtocolSession`, SFEN hashing, `version_tuple`, and reconnect utilities for Wasm targets; used by the browser online battle.
- `notation/` — pure Rust library generating human-readable Japanese kifu notation (e.g. ５八金右, ７六歩) with disambiguation suffixes (右/左/直/上/引/寄) only when needed.
- `notation-wasm/` — thin `wasm-bindgen` cdylib exposing `ja_notation` for browser use; used by the web board for move labels.
- `server/` — Cloudflare Worker + Durable Object managing WebSocket rooms for the browser online battle. Deployed to `fukanzen-shogi-ws.tokuhira.workers.dev`.

**Documentation:**

- `docs/` — rule specifications, implementation guides, change instructions, policy documents.

> Design principle — `engine/` is the **core**; `cli/`, `tui/`, `protocol/`, and `web/` are interchangeable **shells**. The engine never gains I/O, networking, or randomness.

### This Repository — Phases 1–3

**Phase 1 — Rule engine + verification CLI** delivers:

- **A pure Rust rule engine** (`engine/`) — a library crate with zero I/O, no `async`, no RNG, no networking. Its public API is a set of pure functions: `legal_actions`, `resolve`, `check_status`, and serialization helpers.
- **A verification CLI** (`cli/`) — a single-process tool where one person inputs moves for both sides in USI notation, used to manually verify that the engine behaves as specified. Kept as a machine-readable pipe interface; not modified by later phases.
- **A regression test suite** — 41 tests covering all concrete examples from the implementation spec: collision cases (capture, escape, same-square clash, swap, path pass-through, drop clash, promoted-piece reversion, Sengoku Musou swap and drop-clash, both-kings swap draw), move generation (king safety, check evasion, nifu, uchi-fu-dzume, backed-piece prevention, both-kings legal approach, asymmetric swap legality), termination (definite mate, king's death, simultaneous king capture, draw conditions, counter-play backfire, rook pass-through), serialization round-trips, and canonical initial-position verification against the spec's authoritative SFEN.

USI notation is used throughout for moves (`7g7f`, `P*5e`, `2b3a+`). Position serialization follows SFEN with a fixed sentinel (`b`) in place of the turn field, which does not exist in this variant; the canonical initial position is defined by a single `INITIAL_SFEN` constant rather than hardcoded piece placement, making the spec document the single source of truth. Canonical serialization — deterministic board + holdings + move-number bytes — feeds directly into the SHA-256 board-hash layer in Phase 3. A separate content serialization (board + holdings only, no move number) is used for repetition detection.

**Phase 2 — TUI verification desk** adds:

- **A full-screen TUI** (`tui/`) — built on [ratatui](https://ratatui.rs) + crossterm. One person operates both sides with cursor keys and mouse on a single screen, with no secrecy — pure verification mode.
- **Interactive legal-move highlighting** — selecting any piece (on board or in hand) immediately highlights every legal destination for that piece. This makes engine correctness visually inspectable: king-safety exclusions, check-evasion filtering, Sengoku Musou non-application, and piece-drop constraints all appear as the presence or absence of yellow highlights.
- **Simultaneous resolution UI** — build Sente's move, then Gote's move, then press Enter (or click the resolve button) to resolve both atomically. The resolution result (captures, escapes, clashes, Sengoku Musou activations, both-kings swap draw, king deaths) is displayed as text in the info panel.
- **Full mouse support** — board squares, hand-piece areas, the promotion dialog (成る / 成らない), the resolve button, and the game-over popup are all clickable. Clicking a selected piece or hand piece again deselects it.
- **Kumite counter** — turns are displayed as "第N組手" (kumite number) rather than individual half-moves, reflecting that each turn advances both sides simultaneously.
- The engine crate is untouched; the TUI is strictly a new shell on top of the existing public API.

**Phase 3 — Secret simultaneous commitment over TCP** adds:

- **A pure protocol crate** (`protocol/`) — zero I/O, no networking, no RNG. Implements the commit-reveal-ack protocol as a self-contained state machine with five cryptographic properties:
  - *Binding* — a commitment cannot be opened with a different move (SHA-256 of USI action string + nonce).
  - *Hiding* — the commitment reveals nothing about the move; each nonce is freshly generated, making two commits for the same move indistinguishable.
  - *Order* — a reveal is rejected unless both commits have been received, preventing "second-look" cheating.
  - *Board-hash verification* — each reveal includes `SHA-256(canonical_bytes(position))`; a mismatch aborts the game rather than silently producing divergent boards.
  - *Ack synchronization* — both sides must acknowledge each other's reveal before the turn advances, preventing one-move desync from message reordering.
  - Plus: reconnect identity verification (`SHA-256(password)` checked against the stored hash from the game-start handshake) and move-history recovery (kifu position hashes are scanned to find the matching resume point). 31 tests covering all properties.
  - *Version negotiation* (v0.6.0) — immediately after TCP connection, both sides exchange `(rule_version, protocol_version)` tuples; a mismatch causes an immediate abort with a descriptive message before any game state is shared. Clients at v0.6.0 are incompatible with v0.7.0 (protocol version raised to 2; resign added to commit-reveal flow).
- **A TCP network layer** (`tui/src/net.rs`) — 4-byte big-endian length prefix + `serde_json` body; reader in a background thread posting to `mpsc::Receiver<NetEvent>`; the main TUI loop drains it with `try_recv` in a 50 ms poll cycle.
- **An online game state machine** (`tui/src/online.rs`) — `OnlinePhase` tracks `WaitingMyMove → WaitingPeerCommit → WaitingPeerReveal → WaitingPeerAck`; protocol steps auto-advance (commit is sent the moment a move is confirmed; reveal is sent the moment both commits arrive; ack is sent the moment the peer reveal passes verification). The current connection state and protocol phase are shown live in the status bar. On TCP disconnect, reconnection runs non-blocking in a background thread (the Connect side retries every 500 ms for up to 60 seconds); a four-second success banner appears on reconnect, and if the in-progress move was rolled back the user is notified to re-enter it.
- **A portal menu** — the TUI launched without CLI flags shows a mode-selection screen: single-player verification desk, online battle as Sente (Listen), online battle as Gote (Connect), or quit. The online form accepts the port or host:port address and the shared password; from the second game onward the previous values are pre-filled, with automatic adjustment when switching sides (Connect→Listen extracts the port number; Listen→Connect reuses the last known address). An interactive terminal is required; launching via pipe or redirect is rejected at startup.
- **CLI flags** — `--listen PORT` (Sente) and `--connect HOST:PORT` (Gote), with `--secret PASSWORD`, bypass the portal and start online play directly. This mode is retained for scripted or automated setups.

The `protocol/` crate has no dependency on `net.rs` or the TUI; it receives nonces as arguments so tests are fully deterministic. The engine crate remains untouched.

**Web frontend:**

- **An interactive hotsheet board** (`web/`) — HTML/CSS/JS, no build step, no frameworks. One person plays both sides with mouse clicks in sumi ink style. Legal moves are shown as subtle ink dots (v0.5 rules enforced by engine). The kifu backbone accumulates all played moves; ← / → navigation lets you revisit any position and branch from there. Promotion dialog, simultaneous resolution, game-over detection — all engine-driven via Wasm. Deployed to Cloudflare Pages.
- **Browser online battle** (`web/online.js`, `protocol-wasm/`, `server/`) — two players in separate browsers play a fully secret simultaneous game over WebSocket. Each player commits a move without seeing the opponent's; both are revealed only when the other is in. The `protocol-wasm` Wasm module handles commit-reveal, board-hash verification, identity auth, and reconnect recovery — the same protocol logic as the TUI TCP mode, wrapped for the browser. The Cloudflare Worker relays encrypted payloads between clients; a Durable Object per room keeps WebSocket state without a database. Deployed to `fukanzen-shogi-ws.tokuhira.workers.dev`.
- **Version-tuple-stamped archive** (v0.8.0) — the 棋譜を保存 button saves the current game, mid-game or finished, as an archive file embedding `(rule_version, protocol_version, app_version)` alongside the move list. Because collision resolution can change between rule versions (e.g. the Sengoku Musou changes across v0.3–v0.5), a bare move list is not enough to guarantee an old record replays identically — the version tuple lets it be reconstructed under the exact rules it was played with. Backward-compatible with the pre-v0.8.0 plain-kifu format. `engine::archive` is the format's single source of truth; `web/` only handles I/O (download + clipboard copy).
- **Archive retrieval and replay** (v0.8.1) — the 棋譜を読込 button loads a saved archive back in (file picker or pasted text) and replays it through the existing kifu navigation — sumi ink board, ← / → navigation, Japanese notation regenerated from USI. The embedded version tuple and result are shown as a small appreciation line; if the archive's rule version doesn't match the running engine, a plain-language warning is shown without blocking replay (the outcome may not reproduce exactly under a different rule version). Branching from a loaded position still works, same as any other replay. Old bare-kifu files load too. Wire format and rule/protocol versions are unchanged — this step only adds a reader.
- **Wasm wrappers** — `engine-wasm/` exposes the rule engine and the archive format (`build_archive`, `parse_archive`); `protocol-wasm/` exposes the protocol session and the version tuple (`version_tuple`); `notation-wasm/` exposes Japanese move notation. All are thin `wasm-bindgen` cdylibs; the underlying crates are untouched.

### Future directions (not yet implemented)

- **CPU opponent** — search and evaluation function.
- **Broader reach** — smartphone app; multi-language engine re-implementations with cross-diffing against the Rust reference; spectator streaming.

The engine is designed so that all of the above are *shells* around an unchanged core. The engine itself will never gain I/O, networking, or randomness.

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
- **戦国無双特則（4.7）:** 衝突の当事者の**一方のみ**が玉の場合、相討ちを適用しない。玉は相手駒を一方的に取得して衝突マスを占める。スワップ（4.4）と同一マスへの相討ち（4.3）の両方に適用される。同一マス相討ちへの拡張は「玉が安全な空きマスへ退避しても、相手が持ち駒を打ち込めば以前は死んでいた」問題を解消する。着手確定時点での玉の侵入禁止（合法手生成の既存ロジック）により、この特則は後ろ盾のない駒に対してのみ発動する。**v0.5 追加（§4.7）:** 衝突の両当事者がともに玉（両玉スワップ）の場合、双方の戦国無双が拮抗して相殺し、通常の相討ち（4.4）に戻る。両玉が取られ、引き分けとなる（§5.4）。
- **経路の非干渉（4.6）:** 走り駒の経路途中に相手の駒が着地しても干渉しない。判定は最終マスでのみ行う。

**両玉の隣接**は同時着手においては合法に成立する。同一ターンに両者が互いの方向へ歩み寄ると、着手後に両玉が隣接することがある。手番交互の伝統的将棋では起こり得ないが、手番を持たない本ゲームでは構造的に許容される（v0.5 にて正式に認定）。隣接後に両者が互いのマスへ踏み込むと両玉スワップとなり、前述の §4.7 追加条項によって裁定される。

**玉の安全性は確率的になる。** 玉は、着手開始時点で相手の利きのあるマスへは移動できない（伝統的ルールをそのまま引き継ぐ）。ただしこれは着手確定時点の判定であり、相手も同時に動くため、安全だったマスが移動後に危険になりうる。安全なマスへの玉の逃げはそのターン必ず助かる。しかし合駒や反撃で応じることは賭けであり、相手が実際に玉へ向かっていれば取られる。

**終了条件は四種:** （5.1）確定的詰み（着手不能、伝統的詰みと一致）・（5.2）玉の死（衝突解決の結果として玉が取られる）・（5.3）投了・（5.4）引き分け（両玉が同時に取られる）。引き分けは「二枚の駒が互いに相手玉を取る」通常経路のほか、両玉スワップ（§4.7 v0.5）によっても成立する。

> **未確定事項（仕様書 §7）:** 千日手の成立時の扱い、連続王手の千日手の読み替え、打ち歩詰めの厳密な再形式化、入玉宣言法の読み替えはいずれも未確定です。本実装ではこれらを勝手に確定させず、暫定処理とコードコメントによる印として引き継いでいます。

### デモ

Web 盤を公開しています: **[fukanzen-shogi.tokuhira.net](https://fukanzen-shogi.tokuhira.net)**

マウスクリックで先後両方を操作するホットシートモードです。駒を選ぶとその駒の合法手が盤上に淡い墨点で示され（v0.5 ルールを Wasm エンジンが判定）、先手・後手の順に着手を組んで解決します。解決した手は棋譜として積まれ、← / → ボタンで前後に辿れます。過去局面に戻って指し直せば棋譜が分岐します。水墨様式、フレームワーク不要、ビルド不要。

### リポジトリ構成

**Rust ワークスペース（核と殻）:**

- `engine/` — 純粋なルールエンジン。I/O・非同期・乱数・ネットワーク依存なし。公開 API は `legal_actions`・`resolve`・`check_status` と直列化関数群。**核**。
- `cli/` — 機械可読の検証 CLI（テキスト入出力、USI 記法）。以後の段階でも無改変で温存する安定した口。
- `tui/` — 対話的検証卓と**ネットワーク対戦モード**（ratatui + crossterm）。TCP の殻は `tui/src/net.rs`。
- `protocol/` — 通信プロトコルの純粋論理（commit-reveal・盤面ハッシュ検証・Ack・本人認証・中断救済・バージョン交渉）。I/O なし、全テスト決定的。

**Web フロントエンドとサーバー（静的＋サーバーレス）:**

- `web/` — 静的 HTML/CSS/JS 盤。ビルド不要、フレームワーク不要。ホットシート操作とブラウザ秘匿対戦の両モード。`engine-wasm/` と `protocol-wasm/` で Wasm 化した核が駆動。Cloudflare Pages で配信。[web/README.md](web/README.md) を参照。
- `engine-wasm/` — `wasm-bindgen` の薄い cdylib ラッパー。`resolve_ply`・`game_status`・`legal_actions`・`build_archive`・`parse_archive` を SFEN/USI の文字列境界で公開。エンジン本体は無改変。
- `protocol-wasm/` — `wasm-bindgen` の薄い cdylib ラッパー。`ProtocolSession`・SFEN ハッシュ・`version_tuple`・再接続ユーティリティをブラウザ向けに公開。プロトコル本体は無改変。
- `notation/` — 人間可読な日本語棋譜表記を生成する純粋 Rust ライブラリ（例: ５八金右・７六歩）。曖昧さがある場合のみ区別符（右/左/直/上/引/寄）を付加。
- `notation-wasm/` — `wasm-bindgen` の薄い cdylib ラッパー。`ja_notation` をブラウザ向けに公開。Web 盤の着手ラベルに使用。
- `server/` — Cloudflare Worker + Durable Object。ブラウザ秘匿対戦の WebSocket ルーム管理を担う。`fukanzen-shogi-ws.tokuhira.workers.dev` へデプロイ。

**ドキュメント:**

- `docs/` — ルール仕様・実装指示書・変更指示・方針文書。

> 設計哲学「共通の核と交換可能な殻」: `engine/` が核であり、`cli/`・`tui/`・`protocol/`・`web/` はすべて核を包む交換可能な殻。エンジン本体にはいかなる I/O も追加しない。

### このリポジトリ — Phase1・Phase2・Phase3

**Phase1 — ルールエンジン＋検証用 CLI** の成果物:

- **Rust ルールエンジン**（`engine/`）— I/O・非同期・乱数・ネットワーク依存を一切持たない純粋なライブラリクレート。公開 API は `legal_actions`・`resolve`・`check_status` と直列化関数群からなる純粋関数群。
- **検証用 CLI**（`cli/`）— 一人が両陣営の着手を USI 記法で入力し、一局を最後まで進められる検証モード。秘匿性なし・単一プロセス。機械可読の口として以後の段階でも無改変で温存する。
- **回帰テスト群** — 仕様書の具体例を写した 41 本のテスト（衝突解決・戦国無双特則（スワップ＋同一マス打ち込み）・両玉スワップ引き分け（v0.5）・合法手生成・後ろ盾検証・両玉接近合法性・非対称スワップ・終了判定・取り合いの裏目・合駒貫き・両玉同時取得・直列化・初期局面の正本 SFEN 照合）がすべて通過している。

着手記法は USI 準拠（例: `7g7f`、`P*5e`、`2b3a+`）。局面の SFEN 手番フィールドは固定値 `b`（不完全将棋に手番は存在しない）。初期局面は正本 SFEN 定数 `INITIAL_SFEN` をパースして生成し、仕様書が唯一の出典となる。正準直列化（盤面＋持ち駒＋手数）は Phase3 の盤面ハッシュ計算に直結する。千日手検出用の内容直列化（手数除く）とは区別して設計済み。

**Phase2 — TUI 検証卓** の成果物:

- **全画面 TUI**（`tui/`）— [ratatui](https://ratatui.rs)（ターミナル UI ライブラリ）と crossterm による全画面対話インターフェース。一人が先手・後手の両着手をカーソルとマウスで組み立て、同時解決する検証モード（秘匿なし・単一プロセス・単一画面）。
- **合法手インタラクティブ提示**（本段階の目玉機能）— 盤上・駒台の駒を選ぶと、その駒の合法な移動先・打ち先が即座に黄色ハイライトされる。玉の侵入禁止・王手回避による絞り込み・戦国無双の非発動・打ち駒の制約などがハイライトの有無として目視確認できる。
- **同時解決 UI** — 先手の着手を組み、後手の着手を組み、Enter（またはボタンクリック）で両着手を同時に `resolve` する三拍子の操作モデル。解決結果（取得・逃げ・相討ち・戦国無双発動・両玉スワップ引き分け・玉の死）をテキストでパネルに表示する。
- **フルマウス対応** — 盤上マス・駒台・成り選択ダイアログ（成る／成らない）・解決ボタン・ゲームオーバーポップアップがすべてクリッカブル。選択中の駒や駒台駒を再クリックで選択解除。
- **組手カウンタ** — 情報欄のターン表示を「第N組手」に統一。1ターンで先後各1手が同時進むことを「組手」で表現する。
- エンジンクレートは無改変。TUI は既存の公開 API のみを叩く新たな殻として追加した。

**Phase3 — TCP 通信秘匿対戦** の成果物:

- **純粋プロトコルクレート**（`protocol/`）— I/O・乱数なし。commit-reveal-ack プロトコルを状態機械として実装し、9 つの性質を 28 本のテストで保証する:
  - *拘束性* — SHA-256(着手 USI || ノンス) によりコミット後に着手を変更できない
  - *秘匿性* — ノンスが毎回異なるため、同一の着手でもコミット値は別物になる
  - *順序* — 両者のコミットが揃うまでリビールを受理しない（後出し禁止）
  - *盤面ハッシュ相互検証* — 各リビールに `SHA-256(canonical_bytes(局面))` を含め、不一致はアボートにより即時処理
  - *Ack 同期* — 両者が互いのリビールを確認し合うまでターンを進めない（メッセージ順序差によるデシンクを防止）
  - さらに再接続時の本人認証（対局開始時に交換した `SHA-256(パスワード)` との照合）と棋譜ハッシュ照合による再開点特定（RecoverySession）も実装済み。31 本のテストで全性質を保証。
  - *バージョン交渉*（v0.6.0）— TCP 接続直後に双方が `(ルール版, プロトコル版)` タプルを交換し、不一致を即座にアボートとして検出する。v0.6.0 以前のクライアントとは互換性がない。v0.7.0 でプロトコル版が 2 に上がり（投了の commit-reveal 対応追加）、v0.6.0 との対戦互換性もなくなる。
- **TCP 通信殻**（`tui/src/net.rs`）— 4 バイト big-endian 長さプレフィックス + serde_json ボディ。受信スレッドが `mpsc::Sender<NetEvent>` へ送り、TUI メインループが 50 ms ポーリングで `try_recv` する。
- **オンライン状態機械**（`tui/src/online.rs`）— `OnlinePhase`（着手入力中 → コミット待ち → リビール待ち → Ack 待ち）を管理。プロトコルは自動進行（着手確定でコミット送信・両者コミットでリビール送信・リビール検証後に Ack 送信）。接続状態とプロトコルフェーズはステータスバーにリアルタイム表示される。TCP 切断時はバックグラウンドスレッドで非ブロッキング再接続を実行（Connect 側は 500 ms 間隔で最大 60 秒リトライ）。再接続成功時は 4 秒間のバナーを表示し、着手がロールバックされた場合はその旨を通知して再入力を促す。
- **ポータルメニュー** — CLI フラグなしで起動すると、単体検証卓・先手（待ち受け）・後手（接続）・終了を選ぶポータルメニューを表示する。通信対戦フォームではポート番号またはアドレスと共有パスワードを入力でき、二局目以降は前回の入力値をデフォルトとして引き継ぐ（先後逆の場合はポート番号を自動調整）。インタラクティブ端末が必須であり、パイプやリダイレクト経由での起動は起動時に弾かれる。
- **CLI フラグ** — `--listen PORT`（先手）と `--connect HOST:PORT`（後手）、`--secret PASSWORD` を渡すとポータルを経由せず直接対局を開始する。スクリプトや自動化用途向けに引き続きサポート。

`protocol/` クレートは `net.rs` や TUI への依存を持たない。ノンスを引数で受け取るため、すべてのテストが決定的に実行できる。エンジンクレートは無改変のまま。

**Web フロントエンド:**

- **ホットシート操作盤**（`web/`）— HTML/CSS/JS、ビルド不要、フレームワーク不要。水墨様式でマウスクリック着手。合法手を淡い墨点で提示（v0.5 ルール、エンジン判定）。棋譜バックボーンで← / →ナビゲーション・分岐対応。成り選択 UI・同時解決・終局判定をすべて Wasm エンジンが処理。Cloudflare Pages で公開。
- **ブラウザ秘匿対戦**（`web/online.js`・`protocol-wasm/`・`server/`）— 別々のブラウザの二人が WebSocket 経由で本物の秘匿同時対局を行う。各プレイヤーは相手が commit するまで自分の着手だけを持ち、両者が揃った瞬間に同時開示される。`protocol-wasm` Wasm モジュールが commit-reveal・盤面ハッシュ検証・本人認証・再接続救済を処理し、Cloudflare Worker が暗号化済みペイロードを中継する（Durable Object で WebSocket セッションを管理）。TUI の TCP 対戦モードと同一のプロトコル論理をブラウザ向けに再利用。
- **版タプル付きアーカイブ保存**（v0.8.0）— 「棋譜を保存」ボタンで、対局中・終局後を問わず現在の対局を `(ルール版, プロトコル版, アプリ版)` を着手列とともに埋め込んだアーカイブファイルとして保存できる。衝突解決の挙動はルール版ごとに変わりうる（戦国無双特則が v0.3〜v0.5 で変遷したように）ため、着手列だけでは旧記録を同一挙動で再現できる保証がない。版タプルにより、実際に指されたルールのもとで再現できる。v0.8.0 以前の素の棋譜形式とは後方互換。書式の正本は `engine::archive` であり、`web/` は保存・コピーの I/O のみを担う。
- **アーカイブの取り出しと再生**（v0.8.1）— 「棋譜を読込」ボタンで、保存済みアーカイブをファイル選択または貼り付けから読み込み、既存の棋譜ナビ（水墨盤・← / →・日本語表記の再計算）でそのまま再生できる。刻まれた版タプルと結果を鑑賞用の情報行に表示し、読み込んだアーカイブのルール版と現行エンジンが食い違えば、再生を止めずに平易な注意文を表示する（当時と結末が異なりうることを正直に伝える）。読込局面からの分岐再指しも従来どおり可能。旧・素の棋譜形式も読める。書式・ルール版・プロトコル版は不変で、読み手を足すだけの一歩。
- **Wasm ラッパー** — `engine-wasm/` がルールエンジンとアーカイブ書式（`build_archive`・`parse_archive`）を、`protocol-wasm/` がプロトコルセッションと版タプル（`version_tuple`）を、`notation-wasm/` が日本語棋譜表記を公開。すべて `wasm-bindgen` の薄い cdylib であり、各クレート本体は無改変。

### 今後の計画（未実装）

- **CPU 対戦** — 探索・評価関数。
- **展開拡大** — スマートフォンアプリ・多言語実装と Rust 実装に対する差分テスト・観戦配信。

エンジンは「共通の核と交換可能な殻」の設計原則に基づき、これらはすべてエンジンの外側に積む予定である。エンジン本体にはいかなる I/O も追加しない。

---

## Detailed Specification / 詳細仕様

**Rule specifications / ルール仕様:**

- [不完全将棋 ルール仕様 v0.5](docs/不完全将棋_ルール仕様_v0.5.md) — **現行仕様 (current)**。両玉スワップを相討ち引き分けとして正式追加（§4.7 v0.5・§5.4）
- [不完全将棋 ルール仕様 v0.4](docs/不完全将棋_ルール仕様_v0.4.md) — 戦国無双特則（§4.7）をスワップ（4.4）と同一マス相討ち（4.3）の両方に拡張
- [不完全将棋 ルール仕様 v0.3](docs/不完全将棋_ルール仕様_v0.3.md) — 戦国無双特則（§4.7）をスワップ限定で追加
- [不完全将棋 ルール仕様 v0.2](docs/不完全将棋_ルール仕様_v0.2.md) — 初期局面の正本 SFEN と SFEN 手番フィールドの確定的な扱い（`b` 固定）を追記
- [不完全将棋 ルール仕様 v0.1](docs/不完全将棋_ルール仕様_v0.1.md) — 初版ルール定義

**Implementation guides / 実装指示書:**

- [Phase 1 — Rule engine + verification CLI](docs/不完全将棋_実装指示書_Phase1.md)
- [Phase 2 — TUI verification desk](docs/不完全将棋_実装指示書_Phase2_TUI検証卓.md)
- [Phase 3 — TCP secret simultaneous play](docs/不完全将棋_実装指示書_Phase3_TCP通信秘匿対戦.md)
- [Web board — kifu replay, sumi ink style](docs/不完全将棋_実装指示書_最小Web盤_棋譜再生水墨.md)
- [Web board — Wasm engine integration](docs/不完全将棋_実装指示書_Web盤Wasm組み込み.md)
- [Web board — interactive hotsheet, legal-move display](docs/不完全将棋_実装指示書_Web盤操作可能化.md)
- [Web board — browser online battle (Cloudflare DO)](docs/不完全将棋_実装指示書_ブラウザ秘匿対戦_DurableObject.md)
- [GitHub Actions — Windows build](docs/不完全将棋_実装指示書_GitHubActions_Windowsビルド.md)
- [Version management step 1](docs/不完全将棋_実装指示書_バージョン管理Step1.md)
- [README and document structure](docs/不完全将棋_実装指示書_READMEとドキュメント整備_改訂版.md)
- [Version-tuple-stamped archive — Yodogawa step 1](docs/不完全将棋_実装指示書_版タプル付きアーカイブ_淀川第一歩.md) — v0.8.0
- [Archive retrieval and replay — Yodogawa step 2](docs/不完全将棋_実装指示書_アーカイブ取り出しと再生_淀川第二歩.md) — v0.8.1

**Change instructions / 変更指示:**

- [v0.3 — Sengoku Musou exception](docs/不完全将棋_実装変更指示_v0.3_戦国無双.md)
- [v0.4 — Sengoku Musou extension](docs/不完全将棋_実装変更指示_v0.4_戦国無双拡張.md)
- [v0.5 — Both-kings swap draw](docs/不完全将棋_実装変更指示_v0.5_両玉スワップ.md)

**Policy and auxiliary documents / 方針・補助文書:**

- [Version compatibility management](docs/不完全将棋_バージョン互換性管理_方針.md)
- [Version negotiation protocol v0.6.0](docs/不完全将棋_実装指示書_互換性確認プロトコルv0.6.0.md)
- [Game record/data design policy — Yodogawa](docs/不完全将棋_棋譜対局データ設計_方針.md)
- [World view and design policy — Hanzu](docs/不完全将棋_版図_世界観と設計方針.md)
- [Principal design — game-subject schema](docs/不完全将棋_プリンシパル設計_対局主体スキーマ.md)

---

## Build & Run

**Requirements:** Rust stable (edition 2021+), Cargo. An interactive terminal is required for the TUI (pipe/redirect launches are rejected).

```sh
# Build all crates
cargo build

# Run all tests (engine: 41 tests, protocol: 31 tests, notation: 9 tests)
cargo test

# Run the verification CLI (text I/O, machine-readable)
cargo run --bin fukanzen-shogi

# Run the TUI — shows the portal menu (select single-player or online mode)
cargo run --bin fukanzen-shogi-tui

# Online battle — Sente side (direct launch, bypasses portal)
cargo run --bin fukanzen-shogi-tui -- --listen 8765 --secret mypass

# Online battle — Gote side (direct launch, bypasses portal)
cargo run --bin fukanzen-shogi-tui -- --connect 192.168.1.10:8765 --secret mypass

# Display version
cargo run --bin fukanzen-shogi-tui -- --version
```

**Web board** — requires an HTTP server locally (WebAssembly cannot be loaded from `file://`):
```sh
python3 -m http.server 8080 --directory web
# then open http://localhost:8080
```
Live deployment: [fukanzen-shogi.tokuhira.net](https://fukanzen-shogi.tokuhira.net)

**Portal menu** — the no-argument launch opens a menu where you choose single-player verification desk, Sente (Listen), or Gote (Connect). The online form lets you enter the port or address and shared password; after the first game the previous values are pre-filled. When the game ends you return to the portal automatically to start a new one without restarting the binary.

**Direct launch** — passing `--listen` / `--connect` + `--secret` skips the portal and starts an online game immediately, behaving as in earlier releases.

In online mode the commit-reveal-ack exchange is fully automatic: the commit is sent the moment you confirm a move, the reveal is sent when both commits arrive, and the ack is sent once the peer's reveal passes verification. The turn resolves on both screens simultaneously. On disconnect, the Connect side retries for up to 60 seconds in the background; a success banner is displayed and any rolled-back move is flagged for re-entry.

**Pre-built Windows binary** — each push triggers a GitHub Actions workflow that cross-compiles `fukanzen-shogi-tui.exe` for `x86_64-pc-windows-msvc` and uploads it as a build artifact. No Rust toolchain is needed to play on Windows.

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

Keys `u` (undo / cancel input), `n` (new game), `s`/`l` (save/load), `r` (resign), `f`, and `m` apply to the single-player verification desk. In online mode, `q` at the game-over or aborted screen returns to the portal.

Mouse clicks are supported throughout: portal menu items, online connection form fields, board squares, hand-piece areas, the promotion dialog (成る / 成らない), the resolve button, and the game-over popup buttons are all clickable. Clicking a selected piece again deselects it.

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
