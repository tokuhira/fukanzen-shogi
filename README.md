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
- `engine-wasm/` — thin `wasm-bindgen` cdylib exposing `resolve_ply`, `game_status`, `legal_actions`, `build_archive`, `parse_archive`, `evaluate_terminal`, and `max_turns` over a pure SFEN/USI string boundary. Engine core is untouched.
- `protocol-wasm/` — thin `wasm-bindgen` cdylib exposing `ProtocolSession`, SFEN hashing, `version_tuple`, and reconnect utilities for Wasm targets; used by the browser online battle.
- `notation/` — pure Rust library generating human-readable Japanese kifu notation (e.g. ５八金右, ７六歩) with disambiguation suffixes (右/左/直/上/引/寄) only when needed.
- `notation-wasm/` — thin `wasm-bindgen` cdylib exposing `ja_notation` for browser use; used by the web board for move labels.
- `server/` — Cloudflare Worker + Durable Object managing WebSocket rooms for the browser online battle, plus read-only live spectating (`/watch/:token`, backed by a `SPECTATE_TOKENS` KV namespace). Deployed to `fukanzen-shogi-ws.tokuhira.workers.dev`.

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
  - *Version negotiation* (v0.6.0) — immediately after TCP connection, both sides exchange `(rule_version, protocol_version)` tuples; a mismatch causes an immediate abort with a descriptive message before any game state is shared. Clients at v0.6.0 are incompatible with v0.7.0 (protocol version raised to 2; resign added to commit-reveal flow). Protocol version raised again to 3 at v0.10.0 (spectating added; see below) — the negotiation logic itself needed no change, since it already treated any protocol mismatch as incompatible.
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
- **Hardening the archive reader** (v0.8.2) — this was the first feature to parse externally-supplied text (a shared `.kifu` file could come from anyone), so it got a focused security pass: `engine-wasm`'s hand-rolled JSON escaping now escapes control characters per spec (a raw tab/CR in a free-text header field previously produced invalid JSON and threw an uncaught `SyntaxError` in the browser); `parseArchiveText` wraps `JSON.parse` in try/catch as defense in depth; and the loader now rejects archives over 500 plies or 512 KB with a friendly message instead of risking a hung tab on a maliciously large file. The 500-ply cap was a Wasm-boundary safety net, not yet a game rule, at the time.
- **Rule v0.6 — draw as a legitimate outcome, sennichite confirmed, 500-kumite max length** (v0.9.0, `RULE_VERSION` 0.5 → 0.6) — the base shogi rulebook resolves non-decisive games (sennichite, jishōgi, nyūgyoku) by replaying with sides swapped; since this variant has abolished the turn order that swap depends on, all such non-decisive outcomes now map onto a single, first-class **draw** result. Concretely: `engine::terminate::evaluate(kifu)` is the single authority for every terminal condition (definite mate, king's death, sennichite, and the new 500-kumite max-length draw), evaluated in a fixed priority order; the continued-check exception to the base 500-move rule is dropped (checks are probabilistic under simultaneous play, so "every move in the repetition gave check" isn't well-defined) — 500 kumite is now an unconditional cutoff. `engine::archive::ResultKind::MaxTurns` records it; the 500-ply safety net from v0.8.2 now reads this same constant instead of duplicating it. Version negotiation already rejected rule-version mismatches, so online play across 0.5/0.6 correctly refuses to connect. Archive format and protocol version are unchanged — only the rule's meaning moved.
- **Live spectating, step 1: secrecy-boundary routing fix** (v0.9.1) — the Durable Object (`server/src/room.ts`) used to blind-broadcast every message to "every socket but the sender." Ahead of adding spectators, this was replaced with explicit (role, type) routing: sockets are now tagged `player`, the 2-seat cap counts only players, and — this is the load-bearing part — a socket that isn't a tagged player can never receive the raw commit/reveal traffic. Purely a hardening/refactor at this point (no spectator entry point existed yet); verified as a pure regression against the existing 2-player commit-reveal flow.
- **Live spectating, step 2: watch a game live** (v0.9.1) — a room now optionally has read-only observers. The sente-side client broadcasts public-only information once a turn is revealed (`spectate_turn`, plus `spectate_meta` at game start and `spectate_result` at the end) — never commit/reveal/nonce, so the secrecy boundary that commit-reveal already guarantees is never touched, and no artificial delay is needed. The DO tags spectator sockets separately (no 2-seat cap, read-only — any message a spectator sends is discarded) and fans out the public stream to them while accumulating it as the room's live record. Access is via a one-time random token (`/watch/:token`, resolved through a KV `token → roomKey` map) so a spectate link never doubles as a room key a stranger could join with. A newly-connected spectator receives `spectate_init` with everything recorded so far and catches up through the same replay engine step 2 (archive retrieval) already built — sumi ink board, kifu nav, Japanese notation — then follows live turn-by-turn. Players get a shareable watch link in the UI once the token arrives.
- **Server-side archive retrieval** (`GET /room/:key/archive`) — the room's accumulated public-turn record was already persisted to Durable Object storage as a side effect of step 2 (a server-side backstop against "forgot to save"); this step just exposes it over HTTP as a diagnostic-style endpoint, same spirit as `/status`. Verified round-tripping through `build_archive` → `parse_archive` → the existing kifu viewer. Known limitation, by design: a room holds one current record, not a history — starting a new game in the same room key overwrites the previous one the moment `spectate_meta` arrives. No web UI wired to this on purpose; it's meant as an API-level safety net, not a headline feature.
- **Protocol version 2 → 3, closing out Yodogawa step 3** (v0.10.0) — the wire surface grew (spectate messages, DO routing changes), so `PROTOCOL_VERSION` moved to 3, additively; the commit/reveal/hello wire format itself is untouched. `RULE_VERSION` and the archive format stay put. Negotiation needed no changes — it already rejected any protocol mismatch outright, so v3 clients simply refuse to pair with anything older.
- **Fixing a spectator exploit against §1-B** (v0.10.1) — caught in review: `webSocketMessage`'s `request_reset` branch ran *before* the read-only-spectator check, so a spectator socket (untagged as `player`) sending `{type:"request_reset"}` satisfied `other !== ws` against every entry of `getWebSockets("player")` — since a spectator socket was never in that list to begin with — closing both players and clearing `gameStarted`. Anyone holding a watch link could hand-craft this message and blow up someone else's game, no client button needed. Fixed by moving the spectator check to the very top of `webSocketMessage`, before any type-based dispatch, so a spectator's input is discarded unconditionally regardless of `type`. Also fixed a related routing gap while testing this directly (not itself exploitable in production, since real spectators arrive via `/watch/:token`, which bypasses the top-level route match): `/room/:key/spectate` was missing from the Worker's route regex, returning 404 outside that indirection.
- **Record-keeper step 1: giving a finished game its own identity** (v0.10.2) — opening the spectating door in step 3 quietly reopened the original data-loss problem one layer up: a room's record lived in the room's own DO storage, and `spectate_meta` wipes that storage clean the moment a rematch starts in the same room. A finished game you didn't grab in time was simply gone. Fixed by giving a *finished* game its own identity, separate from the room (a rendezvous point that's meant to be reused): the canonical archive text is SHA-256-hashed and content-addressed into a new, permanent KV (`ARCHIVES`) — independent of the room's lifecycle. Abandoned games (no result ever reached) get a random UUID instead, marked `finalized: false`. The invariant holding this together: **a room's record is archived before it's ever wiped or discarded** — enforced at all three points that would otherwise erase it (game end, rematch start, everyone leaving). Retrieval is `GET /archive/:id`, bypassing the room DO entirely; for finalized games, anyone can re-hash the returned text and confirm it matches the id (free tamper-evidence, and the foundation for a future two-witness cross-check). Backward-compatible and additive — `spectate_result`'s new `text` field is optional, so even an old client's abandoned games get caught as fragments.
- **Hardening the archive's ordering and input bounds** (v0.10.3) — a security self-review of the freshly-shipped record-keeper step 1 turned up a regression in its own race-condition fix: `spectate_result` handling set the room's local `archived` flag *before* the (slower, network-bound) write to the `ARCHIVES` KV completed, to narrow a benign duplicate-write race. But if that KV write then failed (oversized payload, transient KV error), the flag would already say "archived" while nothing was actually stored — and since nothing retries once `archived` is true, the record was silently gone forever, exactly the failure mode this whole feature exists to prevent. Fixed by only setting `archived: true` after the KV write succeeds, wrapped in try/catch so a failure degrades to "retry later" instead of "lose it now." Two related gaps closed at the same time: `spectate_turn` had no cap on how many turns a player-tagged socket could append (any client can send this, not just the sente-side convention `web/online.js` follows) — now capped at `MAX_TURNS` (500, matching `engine::terminate::MAX_TURNS`); and `spectate_result`'s `text` had no size limit before being hashed and stored — now capped at 512KB (matching `web/board.js`'s existing `MAX_ARCHIVE_BYTES`), falling back to fragment archiving if exceeded rather than being rejected outright.
- **Record-keeper step 2: invitation and two-witness cross-check** (v0.11.0) — step 1 archived *every* online game automatically; step 2 turns the record-keeper into an explicit, mutually-consented-to participant instead of a silent default. A room now tracks `recording` (default false, reset on every `spectate_meta`); either side can propose `record_invite`, the other `record_accept`s or `record_decline`s, and only once accepted does `record_confirmed` broadcast to both players *and* spectators (transparency about which games get kept). Binding moves off `spectate_result` entirely: on game end, if `recording`, **both** players (not just sente) send `record_testimony{text,kind,outcome}` built from their own independent `buildArchiveText()`. The DO hashes both texts — matching hashes finalize with `witnesses: 2` (two independent honest clients replaying the same commit-reveal transcript necessarily produce byte-identical archives); a mismatch is **never adjudicated** (no referee, per the project's design stance) — instead `record_disagreement{id_a,id_b,id}` surfaces the disagreement to both players and spectators, and both texts are preserved under a `disputed: true` envelope so the evidence isn't lost. If one side disconnects before testifying, the lone testimony still finalizes at `witnesses: 1`. Unrecorded (uninvited) games volatilize exactly as before spectating was added — the room's invariant now reads "archive before wiping, *but only for invited games*." `_archiveCurrentIfNeeded`'s fragment fallback and `_archiveFinalized` are both gated on `recording`. `archived{id}` finally gets its minimal surface in the UI (a status line and a copy-link button), closing the "server broadcasts it, nothing displays it" gap noted in the backlog. `PROTOCOL_VERSION` 3 → 4 for the new message types; `RULE_VERSION` and the archive format are untouched. Two real bugs surfaced only under real-browser testing (not the raw-WebSocket scripts): (1) testimony collection originally cross-checked collected witnesses against `getWebSockets("player")`, but real clients disconnect immediately after sending their testimony, so by the time the second testimony arrived the first socket had already left that list and the match never fired — fixed by keying purely on the `Map`'s size, since a `WebSocket` object identity is stable per connection regardless of whether it's since closed. (2) Even after that fix, both clients closing immediately after testifying meant the `archived`/`record_disagreement` broadcast usually had no one left to reach — the two-witness round trip needs strictly more wall-clock time than the old single-sender path. Fixed by having the client wait for that notification (with a 5s fallback timeout) before disconnecting, rather than disconnecting unconditionally right after sending.
- **Cleanup: dead state left over from step 2** (v0.11.1) — a post-implementation review turned up two write-only leftovers with no reader: `Testimony.kind`/`.outcome` on the DO's in-memory testimony map (the canonical archive text already carries the result; neither the hash comparison nor either archiving path ever looked at these) and `online.js`'s `_recordInvitePending`/`isRecordInvitePending` (tracked and toggled correctly, but nothing ever called the getter — the invite UX is a single blocking `confirm()`, not a persistent pending state). Both removed; no behavior change.
- **Wasm wrappers** — `engine-wasm/` exposes the rule engine, the archive format (`build_archive`, `parse_archive`), and the terminal-state authority (`evaluate_terminal`, `max_turns`); `protocol-wasm/` exposes the protocol session and the version tuple (`version_tuple`); `notation-wasm/` exposes Japanese move notation. All are thin `wasm-bindgen` cdylibs; the underlying crates are untouched.

### Future directions (not yet implemented)

- **CPU opponent** — search and evaluation function.
- **Broader reach** — smartphone app; multi-language engine re-implementations with cross-diffing against the Rust reference.
- **Solo verification board as its own page** — the one-person-plays-both-sides hotseat mode (formerly 新局 on the main board, removed in v0.9.2's button cleanup) deserves a page of its own, separate from the main board's online-play/spectate/archive-review focus. Likely a near-identical page reusing the same rendering code.
- **Interactive tutorial** — distinct from just replaying [`web/sample.kifu`](web/sample.kifu): a guided walkthrough of the rules (simultaneous resolution, Sengoku Musou, etc.) for newcomers.

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
- `server/` — Cloudflare Worker + Durable Object。ブラウザ秘匿対戦の WebSocket ルーム管理に加え、読み取り専用のライブ観戦（`/watch/:token`、`SPECTATE_TOKENS` KV namespace で解決）を担う。`fukanzen-shogi-ws.tokuhira.workers.dev` へデプロイ。

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
  - *バージョン交渉*（v0.6.0）— TCP 接続直後に双方が `(ルール版, プロトコル版)` タプルを交換し、不一致を即座にアボートとして検出する。v0.6.0 以前のクライアントとは互換性がない。v0.7.0 でプロトコル版が 2 に上がり（投了の commit-reveal 対応追加）、v0.6.0 との対戦互換性もなくなる。v0.10.0 でプロトコル版がさらに 3 へ（観戦機能の追加。詳細は後述）——交渉ロジック自体は既にプロトコル版不一致を非互換として弾いていたため無改修。
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
- **読込機構の堅牢化**（v0.8.2）— 外部由来の文字列（誰かから共有された `.kifu` ファイル）を解釈する初の機能だったため、集中的なセキュリティ点検を行った。`engine-wasm` の手組み JSON エスケープが制御文字を仕様どおりエスケープするよう修正（自由記述欄に生のタブ/CR が混入すると不正な JSON になり、ブラウザで捕捉されない `SyntaxError` を投げていた）。`parseArchiveText` は `JSON.parse` を try/catch で多層防御。読込は 500組手・512KB を超えるアーカイブを、巨大な悪意あるファイルでタブが固まるのを避けるため、穏当なメッセージで拒否するようにした。この時点では 500組手の上限は Wasm 境界の安全弁であり、まだ正式なゲームルールではなかった。
- **ルール v0.6 — 引き分けの正式化・千日手確定・最長手数500組手**（v0.9.0、`RULE_VERSION` 0.5 → 0.6）— 基底の将棋規則は非決着（千日手・持将棋・入玉宣言）を先後入れ替えの指し直しで処理するが、本ゲームは手番そのものを廃しているため入れ替える対象がない。ゆえにすべての非決着を、正式な第三の結果である**引き分け**へ写像する。実装面では `engine::terminate::evaluate(kifu)` が終局判定の単一の権威となり、確定的詰み・玉の死・千日手・新設の最長手数（500組手）を決まった優先順序で一元評価する。基底の500手ルールにあった「王手継続中は延長」という例外は、同時着手では王手が確率的で「反復中の手がすべて王手」を定義できないため廃止し、500組手を無条件の打ち切りとした。`engine::archive::ResultKind::MaxTurns` が結果を記録し、v0.8.2 で導入した安全網の500もこの単一の定数を参照するよう寄せて重複を解消した。版交渉は既にルール版不一致を非互換として弾く実装だったため、0.5/0.6混在のオンライン対戦は正しく拒否される。アーカイブ書式・プロトコル版は不変——変わったのはルールの意味だけ。
- **ライブ観戦・第一歩: 秘匿境界のルーティング修正**（v0.9.1）— Durable Object（`server/src/room.ts`）は「送信者以外の全ソケットへ転送」する盲目中継だった。観戦者を迎える前に、これを (役割, 型) による明示ルーティングへ改めた。全ソケットに `player` タグを付与し、2人枠は player のみで計数、そして最も肝心な点として——タグ付けされた player でないソケットには commit/reveal の生トラフィックが絶対に届かない。この時点では観戦者の入口はまだ無く、純粋な堅牢化のみ。既存の2人対局（commit-reveal）に対する回帰確認として検証した。
- **ライブ観戦・第二歩: 対局を生で観る**（v0.9.1）— 部屋に読み取り専用の観戦者を迎えられるようになった。先手側クライアントが、組手が公開された直後にのみ公開情報をブロードキャストする（`spectate_turn`、対局開始時の `spectate_meta`、終局時の `spectate_result`）——commit/reveal/nonce には一切触れないため、commit-reveal が既に保証している秘匿境界を破らず、遅延も不要。DO は観戦者ソケットを別タグで扱い（2人枠の対象外・読み取り専用で観戦者からの送信は破棄）、公開ストリームを観戦者へ fan-out しつつ部屋のライブ記録として蓄積する。アクセスはワンタイムのランダムトークン経由（`/watch/:token`、KV の `token → roomKey` 写像で解決）とし、観戦リンクが誰かの入室鍵を兼ねてしまうことを避ける。新規接続した観戦者は、それまでの記録を `spectate_init` で受け取り、第二歩（アーカイブ取り出し）で作った同じ再生機構——水墨盤・棋譜ナビ・日本語表記——で現局面まで追いつき、以後は一手ずつライブ追従する。プレイヤー側にはトークン到着後、共有可能な観戦リンクが UI に表示される。
- **ボタンの整理**（v0.9.2）— メイン盤のボタン列が5〜6個に膨らみ、狭い画面では折り返して窮屈になっていた。`デモ局面`・`新局` をこのページから削除。デモの役割（動く実例を見せる）は、アプリ内蔵のハードコードされたデモではなく、[`web/sample.kifu`](web/sample.kifu)（「棋譜を読込」から読み込める実物のアーカイブ）が担う。`新局` の役割（一人で両陣営を指す検証盤）は、メイン盤（オンライン対戦・観戦・棋譜鑑賞が主眼）とは異なる関心事のため、いずれ別ページへ切り出す候補として記録した（下記「今後の計画」、未着手）。ローカルのホットシート着手ロジック自体は無改変で、このページ上に専用のリセット導線が無いだけ。終局後に「対戦」で再戦を始めると自動的に状態がリセットされるようになり（旧「新局」がこのケースで担っていた役割を引き継ぐ）、観戦セッションの離脱には専用の「観戦をやめる」ボタンを新設した。
- **サーバ側アーカイブの取り出し**（`GET /room/:key/archive`）— 部屋に蓄積された公開組手の記録は、第二歩の実装時点で既に Durable Object のストレージへ永続化されていた（「保存し損ね」に対するサーバ側の保険）。本ステップは、それを `/status` と同じ診断系エンドポイントとして HTTP 越しに公開しただけ。`build_archive` → `parse_archive` → 既存の棋譜ビューアで実際に往復再生できることを確認済み。既知の制約（意図した設計）: 部屋が保持するのは単一の最新レコードであり履歴ではない——同じルームキーで新しい対局を始めると `spectate_meta` 受信時に前回の記録が上書きされる。意図的にUI導線は付けていない。あくまでAPIレベルの保険であり、目玉機能ではない。
- **プロトコル版 2 → 3、淀川第三歩の完了**（v0.10.0）— ワイヤ表面が広がった（観戦系メッセージ・DO のルーティング変更）ため、`PROTOCOL_VERSION` を加算的に 3 へ。commit/reveal/hello のワイヤ形式自体は無改変。`RULE_VERSION` とアーカイブ書式は据え置き。版交渉は無改修で済んだ——既にプロトコル版の不一致を無条件に弾く実装だったため、v3 クライアントは旧版とは単純に対戦不可となる。
- **観戦者による §1-B 違反の修正**（v0.10.1）— レビューで発覚: `webSocketMessage` の `request_reset` 分岐が、読み取り専用の観戦者チェックより**先**にあった。観戦者ソケット（`player` タグを持たない）が `{type:"request_reset"}` を送ると、`getWebSockets("player")` の全エントリに対して `other !== ws` が成立してしまう（観戦ソケットはそもそもそのリストに含まれないため）——結果、両プレイヤーが切断され `gameStarted` がクリアされ、対局が壊れる。観戦リンクを持つ誰かがこのメッセージを手組みするだけで、他人の対局を壊せてしまっていた。クライアントにボタンが無いことは対策にならない。観戦者チェックを `webSocketMessage` の先頭、型による分岐より前に移し、`type` を問わず無条件に入力を破棄するよう修正した。あわせて、これを直接検証する過程で見つかった関連のルーティングの穴も修正: `/room/:key/spectate` が Worker 側のルート正規表現に含まれておらず 404 になっていた（本番の観戦者は `/watch/:token` 経由でこのトップレベルのルート判定を迂回するため、それ自体が悪用可能だったわけではない）。
- **記録係一段目: 対局それ自身の身元**（v0.10.2）— 第三歩で観戦の口を開けたことで、データ喪失問題が一層上で静かに再発していた——部屋の記録は部屋自身の DO ストレージに住み、同じ部屋で再戦が始まった瞬間 `spectate_meta` がそのストレージを拭ってしまうため、取りそびれた終局記録はただ消える。対局を、使い回されて当然の殻である部屋から解き、それ自身の身元を持たせて修正した: 正準アーカイブ本文を SHA-256 し、その内容ハッシュで新設の永続 KV（`ARCHIVES`）へ content-address する——部屋のライフサイクルから独立。終局に至らず放棄された対局はランダム UUID を持ち `finalized:false` とする。これを支える不変条件: **部屋の記録は、書庫へ綴じ終えてからでなければ拭かない・捨てない**——これを消しうる三つの契機（終局・再戦開始・全員離脱）すべてで強制する。取り出しは `GET /archive/:id`（部屋 DO を一切介さない）。確定局は、返ってきた本文を誰でも再ハッシュして id と照合できる（無償の改竄検知であり、将来の二証人交差確認の土台）。加算的・後方互換——`spectate_result` の新しい `text` フィールドは任意なので、旧クライアントの放棄局も断片として救われる。
- **書庫の順序と入力上限の締め直し**（v0.10.3）— 出したばかりの記録係一段目に対するセキュリティ自己レビューで、自身の競合修正が退行を生んでいることが発覚。`spectate_result` の処理は、無害な二重書き込みの競合窓を縮めるために、部屋のローカルな `archived` フラグを（ネットワーク越しで遅い）`ARCHIVES` KV への書き込みが完了する**前**に立てていた。しかしその KV 書き込みが失敗すると（本文が大きすぎる・KV の一時障害など）、フラグは「綴じ済み」を名乗るのに実体は何も保存されていない状態になり、`archived` が真になった後は誰も再試行しないため、この書庫機能そのものが防ごうとしていた**サイレントな記録喪失**が起きうる作りになっていた。KV 書き込みが成功した後にのみ `archived: true` を立て、try/catch で包んで失敗時は「後で再試行できる状態」に留めるよう修正。同じレビューで見つかった関連の2点も締めた: `spectate_turn` に着手件数の上限がなく、（`web/online.js` の「送るのは先手側のみ」という規約に従わない、生の・改造されたクライアントも含め）任意の player タグ付きソケットが際限なく偽の着手を送り続けられた——`MAX_TURNS`（500、`engine::terminate::MAX_TURNS` と同値）で上限化。`spectate_result` の `text` にもサイズ上限がなくハッシュ化・保存されていた——`web/board.js` の既存 `MAX_ARCHIVE_BYTES` と同じ 512KB を上限とし、超過時は拒否ではなく断片綴じへ自然にフォールバックする。
- **記録係二段目: 招待と二証人の交差確認**（v0.11.0）— 一段目はオンライン対局を**すべて自動で**綴じていた。二段目はこれを、相互同意で明示的に招かれる参加者へ変える。部屋は `recording`（既定 false、`spectate_meta` ごとにリセット）を持ち、どちらの陣営からでも `record_invite` を提案でき、相手が `record_accept`/`record_decline` で応じ、承諾されて初めて `record_confirmed` が両プレイヤー**と観戦者**へ放送される（どの対局が残されるか透明に示す）。綴じの契機は `spectate_result` から完全に離れた: 終局時、`recording` なら**両プレイヤー**（先手だけでなく）が、各自の独立した `buildArchiveText()` から `record_testimony{text,kind,outcome}` を証言として送る。DO は両本文をハッシュ化して突き合わせ——一致すれば `witnesses:2` で確定綴じ（同じ commit-reveal の記録を独立に再生した正直な二者は、必然的にバイト一致するアーカイブを作る）。不一致は**決して裁定しない**（審判を置かない、という本プロジェクトの設計方針どおり）——代わりに `record_disagreement{id_a,id_b,id}` が両プレイヤーと観戦者へ食い違いを surface し、両証言を `disputed:true` の envelope で保存して証拠を失わない。相手が証言前に離脱していれば、片方の証言のみで `witnesses:1` として確定綴じする。招かれていない（未招待の）対局は、観戦機能が入る前とまったく同じく揮発する——部屋の不変条件はいまや「拭く前に綴じる、ただし招かれた対局のみ」。`_archiveCurrentIfNeeded` の断片フォールバックと `_archiveFinalized` はどちらも `recording` でゲートする。`archived{id}` は本段で初めて画面に最小限の surface（状態表示行とコピー用リンクボタン）を持ち、バックログにあった「サーバは配信しているのに何も表示しない」という穴を閉じた。新しいメッセージ型のため `PROTOCOL_VERSION` は 3→4（`RULE_VERSION` とアーカイブ書式は無改変）。実ブラウザでの検証で初めて（生WebSocketのスクリプトでは見えない形で）2件の実バグが発覚した: (1) 証言の収集がもともと `getWebSockets("player")` との突き合わせに依存していたが、実際のクライアントは証言送信直後に自分から切断するため、二人目の証言が届く頃には一人目の ws がそのリストから既に外れており、一致判定が起きなかった——`WebSocket` オブジェクトの同一性は、その接続が閉じた後も安定しているので、`Map` の `size` だけで判定するよう修正。(2) その修正後も、両者が証言直後に即切断する挙動のせいで、`archived`/`record_disagreement` の放送が届く相手がほぼ誰もいない状態になっていた——二証人の往復には旧来の単送信経路より確実に長い実時間がかかるため。切断を、通知を受け取るか保険の5秒タイムアウトまで待つよう修正した。
- **締め直し: 二段目が残した書き込み専用の状態**（v0.11.1）— 実装直後の振り返りで、読み手のいない2箇所が見つかった: DO のメモリ内証言 Map が持つ `Testimony.kind`/`.outcome`（正準本文に既に結果が埋め込まれており、ハッシュ比較・どちらの綴じ経路も参照しない）、`online.js` の `_recordInvitePending`/`isRecordInvitePending`（立て下げは正しく行われるが、getter を呼ぶ箇所が一つもない——招待UIは一発の `confirm()` であり、持続する保留状態を持たない）。両方削除、挙動は無変更。
- **Wasm ラッパー** — `engine-wasm/` がルールエンジン・アーカイブ書式（`build_archive`・`parse_archive`）・終局判定の権威（`evaluate_terminal`・`max_turns`）を、`protocol-wasm/` がプロトコルセッションと版タプル（`version_tuple`）を、`notation-wasm/` が日本語棋譜表記を公開。すべて `wasm-bindgen` の薄い cdylib であり、各クレート本体は無改変。

### 今後の計画（未実装）

- **CPU 対戦** — 探索・評価関数。
- **展開拡大** — スマートフォンアプリ・多言語実装と Rust 実装に対する差分テスト。
- **一人指し検証盤を独立ページへ** — 一人で両陣営を指すホットシートモード（v0.9.2 のボタン整理でメイン盤から外した旧「新局」）は、メイン盤（オンライン対戦・観戦・棋譜鑑賞が主眼）とは別の関心事として、独立したページに切り出すのが良さそう。おそらく同じ描画コードを再利用したほぼ同一のページになる。
- **インタラクティブなチュートリアル** — [`web/sample.kifu`](web/sample.kifu) の再生だけでは伝わらない、ルール（同時解決・戦国無双特則など）を段階的に案内する初心者向けガイド。

エンジンは「共通の核と交換可能な殻」の設計原則に基づき、これらはすべてエンジンの外側に積む予定である。エンジン本体にはいかなる I/O も追加しない。

---

## Detailed Specification / 詳細仕様

**Rule specifications / ルール仕様:**

- [不完全将棋 ルール仕様 v0.6](docs/不完全将棋_ルール仕様_v0.6.md) — **現行仕様 (current)**。引き分けを正当な第三の結果として定義、千日手を引き分けとして確定、最長手数500組手を新設（§5.0・5.6・5.7・5.8）
- [不完全将棋 ルール仕様 v0.5](docs/不完全将棋_ルール仕様_v0.5.md) — 両玉スワップを相討ち引き分けとして正式追加（§4.7 v0.5・§5.4）
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
- [Terminal-state revision — rule v0.6](docs/不完全将棋_実装指示書_終局判定の改訂_ルールv0.6.md) — v0.9.0
- [Live spectating and server-side archive — Yodogawa step 3](docs/不完全将棋_実装指示書_ライブ観戦とサーバ側アーカイブ_淀川第三歩.md) — v0.9.1–v0.10.0
- [Game persistence and identity — record-keeper step 1](docs/不完全将棋_実装指示書_対局の永続と身元_記録係一段目.md) — v0.10.2

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
- [Backlog — seeds and open questions](docs/不完全将棋_バックログ_伏線と未決.md) — living index of deferred work; consult and update alongside ongoing development rather than letting pending items scatter across chats.

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
