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
- **Live spectating** (v0.9.1) — once a room is playing, the sente-side client shares a one-time watch link (`?watch=<token>`). Anyone with the link joins read-only via a separate Durable Object socket tag — no room key, no way to interfere, and no commit/reveal traffic ever reaches them (only post-reveal turns, exactly like the archive format). A spectator catches up through the same replay engine as loading a saved archive, then follows the game live, turn by turn. The room's public-turn record persists server-side and can be pulled via `GET /room/:key/archive` (API only, no UI wired to it — a room holds one current record, overwritten on the next game in the same key). Protocol version is 3 as of v0.10.0 (wire surface grew to include spectating; commit/reveal/hello themselves are unchanged).
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
wasm-pack build engine-wasm    --target web --out-dir ../web/wasm           --no-pack
wasm-pack build protocol-wasm  --target web --out-dir ../web/protocol-wasm  --no-pack
wasm-pack build notation-wasm  --target web --out-dir ../web/notation-wasm  --no-pack

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
- **ライブ観戦**（v0.9.1）— 対局が始まると、先手側クライアントがワンタイムの観戦リンク（`?watch=<token>`）を表示する。リンクを持つ誰でも、別タグの読み取り専用ソケットで入室できる——入室鍵は不要、対局への介入不可、commit/reveal のトラフィックも一切届かない（アーカイブ書式と同じく、公開された組手のみ）。観戦者は保存済みアーカイブの読込と同じ再生機構で現局面まで追いつき、以後は一手ずつライブで追従する。部屋の公開組手記録はサーバ側にも永続化されており `GET /room/:key/archive` で取得できる（UI導線なしのAPIのみ。部屋が保持するのは単一の最新レコードで、同じキーでの次の対局で上書きされる）。プロトコル版は v0.10.0 時点で 3（観戦機能によりワイヤ表面が拡大。commit/reveal/hello 自体は無改変）。
- **← / → ナビ** — 過去局面へ戻ってそこから指し直すと棋譜が分岐。
- **成りダイアログ** — 任意成りが可能な着手で表示。
- **日本語棋譜表記** — ５八金右・７六歩など、曖昧さがある場合のみ区別符（右・左・直・上・引・寄）を付加。
- **デモ局面 / 新局** ボタンで 6 組手デモ局を再生、または初期局面にリセット。
- **棋譜を保存** — 対局中・終局後を問わず、版タプル付きアーカイブ書式（`.kifu`、ダウンロード＋クリップボードコピー）で現在の対局を保存。`(ルール版, プロトコル版)` を埋め込むため、ルール変更後も旧記録を正しく再現できる。
- **棋譜を読込** — 保存したアーカイブをファイル選択または貼り付けで読み込み、既存の棋譜ナビ（← / →・水墨盤・日本語表記）でそのまま鑑賞できる。読み込んだ局面から盤クリックで分岐再指しも可能。刻まれた版タプルと結果を表示し、読み込んだアーカイブのルール版と現行エンジンが食い違う場合は平易な注意文を表示する（再生自体は止めない）。v0.8.0 より前の素の棋譜ファイルも読み込める。外部由来の文字列を扱う機能のため、512KBを超えるアーカイブは穏当なメッセージで拒否する（クライアント側の安全弁）。自由記述欄への制御文字混入に対する JSON エスケープも強化済み（v0.8.2）。500組手の上限は、v0.9.0 でハードコードの重複をやめ、実際のルール定数（`engine::terminate::MAX_TURNS`）を直接参照するようになった——正当な対局はこれを超えない。

全局面は Wasm エンジンがブラウザ上でリアルタイム計算。ハードコードされた局面データはない。

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
wasm-pack build engine-wasm    --target web --out-dir ../web/wasm           --no-pack
wasm-pack build protocol-wasm  --target web --out-dir ../web/protocol-wasm  --no-pack
wasm-pack build notation-wasm  --target web --out-dir ../web/notation-wasm  --no-pack

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
