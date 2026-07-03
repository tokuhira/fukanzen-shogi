# web/ — Fukanzen Shogi Web Board / 不完全将棋 Web 盤

## English

A static web board for Fukanzen Shogi, backed by a Cloudflare Workers serverless layer for online play.

**Live (production):** [fukanzen-shogi.tokuhira.net](https://fukanzen-shogi.tokuhira.net)  
**Dev:** [fukanzen-shogi.pages.dev](https://fukanzen-shogi.pages.dev)

### Current state

Interactive board with offline single-player and online browser-vs-browser battle.

- **Click a piece** to see legal moves as subtle ink dots on the board (v0.6 rules enforced by engine — draw is a first-class result: definite mate, king's death, sennichite, and the new 500-kumite max length all end the game and are recorded correctly, not just mate/king's death/resignation as before).
- **Offline mode** — one person plays both sides with mouse/click; both moves commit simultaneously per turn.
- **Online mode** — two players in the same room via commit-reveal: each side commits secretly, then both are revealed simultaneously.
- **Live spectating** (v0.9.1) — once a room is playing, the sente-side client shares a one-time watch link (`?watch=<token>`). Anyone with the link joins read-only via a separate Durable Object socket tag — no room key, no way to interfere, and no commit/reveal traffic ever reaches them (only post-reveal turns, exactly like the archive format). A spectator catches up through the same replay engine as loading a saved archive, then follows the game live, turn by turn. The room's public-turn record persists server-side and can be pulled via `GET /room/:key/archive` (API only, no UI wired to it — this is the room's *current* record, and a room is a reused rendezvous point; see below for the permanent record). Protocol version is 3 as of v0.10.0 (wire surface grew to include spectating; commit/reveal/hello themselves are unchanged). **v0.10.1** fixed a real hole in the read-only guarantee: `request_reset` was dispatched before the spectator check in `webSocketMessage`, so a spectator socket (never tagged `player`) sending `{type:"request_reset"}` satisfied the "close every other player" loop and could blow up someone else's game with a hand-crafted message — no client button needed. The spectator check now runs first, unconditionally, before any type-based dispatch. **v0.10.2** closed the data-loss hole that spectating quietly reopened: a room's record used to live only in that room's own storage, wiped the moment a rematch started — so a finished game you didn't grab in time was gone. Finished games now get their own permanent identity (a SHA-256 of the canonical archive text) in a separate KV store, independent of the room; retrievable forever via `GET /archive/:id`, and anyone can re-hash the returned text to confirm it matches. `sendSpectateResult` now optionally carries the full archive text (`buildArchiveText()`) for this; abandoned games without a result still get saved as a fragment under a random id. **v0.10.3** fixed a regression in v0.10.2's own race-condition fix (the `archived` flag was being set before the KV write it guards completed, so a failed write could silently masquerade as archived) and added server-side bounds that were missing on the new channels: `spectate_turn` is capped at 500 entries, `spectate_result`'s `text` at 512KB before it's hashed and stored.
- **Navigate** with ← / → buttons or arrow keys; revisiting any past position and playing from there branches the kifu.
- **Promotion dialog** — appears on moves that can optionally promote.
- **Japanese notation** — move labels use human-readable kifu notation (e.g. ５八金右, ７六歩) with disambiguation suffixes only when needed.
- **棋譜を保存** — save the current game (mid-game or finished) as a version-tuple-stamped archive file (`.kifu`, download + clipboard copy). The archive embeds `(rule_version, protocol_version)` so old records replay correctly even after rule changes.
- **棋譜を読込** — load a saved archive back in, via file picker or pasted text. Replays through the same navigation (← / →, sumi ink, Japanese notation); branching from a past position still works. The embedded version tuple and result are shown; if the archive's rule version doesn't match the running engine, a plain-language warning is shown (replay still proceeds — it just may not reproduce the original outcome exactly). Old bare-kifu files (pre-v0.8.0) still load. Since this reads externally-supplied text, it rejects archives over 512 KB with a friendly message (client-side hygiene limit), and JSON escaping was hardened against control characters in free-text header fields (v0.8.2). The 500-ply cap now reads the real rule constant (`engine::terminate::MAX_TURNS`, v0.9.0) rather than a hardcoded duplicate — no legitimate game can exceed it.

All positions are computed by the Wasm engine at runtime. No hardcoded SFEN data.

**Button cleanup** (v0.9.2) — the main board's button row had grown to 5–6 buttons and was wrapping awkwardly on narrow screens. `デモ局面` and `新局` were removed from this page: the demo's role (a working example to look at) is now served by [`web/sample.kifu`](sample.kifu) — a real archive, loadable via 棋譜を読込 — rather than a hardcoded in-app demo; and `新局`'s role (a one-person verification board, playing both sides yourself) is earmarked to move to its own, mostly-identical page later, since it's a different concern from this page's online-play/spectate/archive-review focus (not yet built — noted as a future direction below). The underlying local hotseat click-to-move logic is untouched, just without a dedicated reset entry point on this page for now. Starting a new online game (対戦) after a finished one now resets state automatically, taking over what `新局` used to do for that case; leaving a spectator session got a dedicated 観戦をやめる button.

### Design boundary

`board.js` uses three Wasm modules:

| Module | Location | Role |
|---|---|---|
| `engine-wasm` | `web/wasm/` | `resolve_ply`, `game_status`, `legal_actions`, `build_archive`, `parse_archive` — rule engine + archive format |
| `protocol-wasm` | `web/protocol-wasm/` | commit-reveal message encoding/decoding (online play), `version_tuple` |
| `notation-wasm` | `web/notation-wasm/` | `ja_notation` — human-readable Japanese kifu notation |

The engine is the sole source of rule truth; the JS layer handles only UI state and rendering.
The serverless backend (`server/`) runs on Cloudflare Durable Objects and handles room state for online play.

### How to run locally

WebAssembly requires HTTP (not `file://`). Use any local HTTP server:

```sh
python3 -m http.server 8080 --directory web
# then open http://localhost:8080
```

Online play requires the Cloudflare Workers backend. For local online testing, see `server/README.md`.

### Rebuild Wasm

Run from the repository root. After each build, remove the `.gitignore` that `wasm-pack` auto-generates.

```sh
wasm-pack build engine-wasm    --target web --out-dir ../web/wasm           --release
wasm-pack build protocol-wasm  --target web --out-dir ../web/protocol-wasm  --release
wasm-pack build notation-wasm  --target web --out-dir ../web/notation-wasm  --release

rm -f web/wasm/.gitignore web/protocol-wasm/.gitignore web/notation-wasm/.gitignore
```

### Deploy

```sh
# First time: authenticate
npx wrangler login

# Deploy web/ to Cloudflare Pages
npx wrangler pages deploy
```

Config: `wrangler.toml` at repository root (`pages_build_output_dir = "web"`).

---

## 日本語

不完全将棋の Web 盤。Cloudflare Workers によるサーバーレス層でブラウザ間オンライン対戦にも対応。

**本番（公開）:** [fukanzen-shogi.tokuhira.net](https://fukanzen-shogi.tokuhira.net)  
**開発:** [fukanzen-shogi.pages.dev](https://fukanzen-shogi.pages.dev)

### 現状

オフラインのひとり操作とブラウザ間オンライン対戦の両方に対応したインタラクティブ盤。

- **駒をクリック**すると合法手が淡い点で表示される（エンジンが v0.6 ルールを適用。引き分けが正式な結果に——確定的詰み・玉の死・千日手・新設の最長手数500組手のいずれも終局として正しく検出・記録される。以前は詰み・玉の死・投了しか終局にならなかった）。
- **オフラインモード** — 一人で先後両方を操作。毎ターン両着手を同時確定。
- **オンラインモード** — 同一ルームの 2 名がコミット秘匿→同時開示方式で対戦（秘密情報保護）。
- **ライブ観戦**（v0.9.1）— 対局が始まると、先手側クライアントがワンタイムの観戦リンク（`?watch=<token>`）を表示する。リンクを持つ誰でも、別タグの読み取り専用ソケットで入室できる——入室鍵は不要、対局への介入不可、commit/reveal のトラフィックも一切届かない（アーカイブ書式と同じく、公開された組手のみ）。観戦者は保存済みアーカイブの読込と同じ再生機構で現局面まで追いつき、以後は一手ずつライブで追従する。部屋の公開組手記録はサーバ側にも永続化されており `GET /room/:key/archive` で取得できる（UI導線なしのAPIのみ。これは部屋の**いまの**記録であり、部屋自体は使い回される落ち合い点——恒久の記録は下記参照）。プロトコル版は v0.10.0 時点で 3（観戦機能によりワイヤ表面が拡大。commit/reveal/hello 自体は無改変）。**v0.10.1** で読み取り専用の保証にあった実際の穴を修正: `webSocketMessage` 内で `request_reset` が観戦者チェックより先に処理されていたため、観戦者ソケット（`player` タグを持たない）が `{type:"request_reset"}` を送ると「他の全プレイヤーを閉じる」ループの条件を満たしてしまい、手組みメッセージ一つで他人の対局を壊せていた（クライアントにボタンは不要）。観戦者チェックを型による分岐より前・無条件に実行するよう修正済み。**v0.10.2** で、観戦の口を開けたことで再発していたデータ喪失の穴を閉じた: 部屋の記録は部屋自身のストレージにしか無く、再戦の瞬間に拭かれていたため、取りそびれた終局記録は消えていた。確定局は正準アーカイブ本文の SHA-256 を身元として、部屋とは別の永続 KV へ恒久的に綴じるようになり、`GET /archive/:id` でいつでも取り出せる（誰でも本文を再ハッシュして id と照合できる）。`sendSpectateResult` はこのために完成本文（`buildArchiveText()`）を任意で同梱するようになり、未終局のまま放棄された対局も暫定IDの断片として救われる。**v0.10.3** で、v0.10.2 自身の競合修正が生んでいた退行を修正（`archived` フラグをそれが守るべき KV 書き込みの完了より前に立てていたため、書き込み失敗がサイレントに「綴じ済み」を騙りうる作りだった）。あわせて、新設したチャネルに欠けていたサーバ側の上限も追加: `spectate_turn` は 500 件、`spectate_result` の `text` はハッシュ化・保存の前に 512KB を上限とする。
- **← / → ナビ** — 過去局面へ戻ってそこから指し直すと棋譜が分岐。
- **成りダイアログ** — 任意成りが可能な着手で表示。
- **日本語棋譜表記** — ５八金右・７六歩など、曖昧さがある場合のみ区別符（右・左・直・上・引・寄）を付加。
- **棋譜を保存** — 対局中・終局後を問わず、版タプル付きアーカイブ書式（`.kifu`、ダウンロード＋クリップボードコピー）で現在の対局を保存。`(ルール版, プロトコル版)` を埋め込むため、ルール変更後も旧記録を正しく再現できる。
- **棋譜を読込** — 保存したアーカイブをファイル選択または貼り付けで読み込み、既存の棋譜ナビ（← / →・水墨盤・日本語表記）でそのまま鑑賞できる。読み込んだ局面から盤クリックで分岐再指しも可能。刻まれた版タプルと結果を表示し、読み込んだアーカイブのルール版と現行エンジンが食い違う場合は平易な注意文を表示する（再生自体は止めない）。v0.8.0 より前の素の棋譜ファイルも読み込める。外部由来の文字列を扱う機能のため、512KBを超えるアーカイブは穏当なメッセージで拒否する（クライアント側の安全弁）。自由記述欄への制御文字混入に対する JSON エスケープも強化済み（v0.8.2）。500組手の上限は、v0.9.0 でハードコードの重複をやめ、実際のルール定数（`engine::terminate::MAX_TURNS`）を直接参照するようになった——正当な対局はこれを超えない。

全局面は Wasm エンジンがブラウザ上でリアルタイム計算。ハードコードされた局面データはない。

**ボタンの整理**（v0.9.2）— メイン盤のボタン列が5〜6個に膨らみ、狭い画面では折り返して窮屈になっていた。`デモ局面`・`新局` をこのページから削除。デモの役割（動く実例を見せる）は、アプリ内蔵のハードコードされたデモではなく、[`web/sample.kifu`](sample.kifu)（「棋譜を読込」から読み込める実物のアーカイブ）が担う。`新局` の役割（一人で両陣営を指す検証盤）は、メイン盤（オンライン対戦・観戦・棋譜鑑賞が主眼）とは異なる関心事のため、いずれ別ページへ切り出す候補として記録した（バックログ参照、未着手）。ローカルのホットシート着手ロジック自体は無改変で、このページ上に専用のリセット導線が無いだけ。終局後に「対戦」で再戦を始めると自動的に状態がリセットされるようになり（旧「新局」がこのケースで担っていた役割を引き継ぐ）、観戦セッションの離脱には専用の「観戦をやめる」ボタンを新設した。

### 設計の境界

`board.js` が使う Wasm モジュールは 3 つ：

| モジュール | 配置先 | 役割 |
|---|---|---|
| `engine-wasm` | `web/wasm/` | `resolve_ply` / `game_status` / `legal_actions` / `build_archive` / `parse_archive` — ルールエンジン＋アーカイブ書式 |
| `protocol-wasm` | `web/protocol-wasm/` | コミット秘匿プロトコルのメッセージ符号化（オンライン対戦）、`version_tuple` |
| `notation-wasm` | `web/notation-wasm/` | `ja_notation` — 日本語棋譜表記生成 |

ルールの唯一の正源は engine-wasm。JS 層は UI 状態と描画のみ担当。
サーバー側（`server/`）は Cloudflare Durable Objects 上で稼働し、オンラインのルーム状態を管理。

### ローカルで動かす

Wasm は `file://` では動かないので HTTP サーバーが必要：

```sh
python3 -m http.server 8080 --directory web
# ブラウザで http://localhost:8080 を開く
```

オンライン対戦のローカルテストには Cloudflare Workers バックエンドが必要。`server/README.md` を参照。

### Wasm 再ビルド

リポジトリルートから実行。`wasm-pack` が自動生成する `.gitignore` を毎回削除する。

```sh
wasm-pack build engine-wasm    --target web --out-dir ../web/wasm           --release
wasm-pack build protocol-wasm  --target web --out-dir ../web/protocol-wasm  --release
wasm-pack build notation-wasm  --target web --out-dir ../web/notation-wasm  --release

rm -f web/wasm/.gitignore web/protocol-wasm/.gitignore web/notation-wasm/.gitignore
```

### デプロイ

```sh
# 初回認証
npx wrangler login

# web/ を Cloudflare Pages へデプロイ
npx wrangler pages deploy
```

設定: リポジトリルートの `wrangler.toml`（`pages_build_output_dir = "web"`）。
