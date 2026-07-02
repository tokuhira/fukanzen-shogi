# web/ — Fukanzen Shogi Web Board / 不完全将棋 Web 盤

## English

A static web board for Fukanzen Shogi, backed by a Cloudflare Workers serverless layer for online play.

**Live (production):** [fukanzen-shogi.tokuhira.net](https://fukanzen-shogi.tokuhira.net)  
**Dev:** [fukanzen-shogi.pages.dev](https://fukanzen-shogi.pages.dev)

### Current state

Interactive board with offline single-player and online browser-vs-browser battle.

- **Click a piece** to see legal moves as subtle ink dots on the board (v0.5 rules enforced by engine).
- **Offline mode** — one person plays both sides with mouse/click; both moves commit simultaneously per turn.
- **Online mode** — two players in the same room via commit-reveal: each side commits secretly, then both are revealed simultaneously.
- **Navigate** with ← / → buttons or arrow keys; revisiting any past position and playing from there branches the kifu.
- **Promotion dialog** — appears on moves that can optionally promote.
- **Japanese notation** — move labels use human-readable kifu notation (e.g. ５八金右, ７六歩) with disambiguation suffixes only when needed.
- **デモ局面 / 新局** buttons load the built-in 6-turn demo or reset to a blank board.
- **棋譜を保存** — save the current game (mid-game or finished) as a version-tuple-stamped archive file (`.kifu`, download + clipboard copy). The archive embeds `(rule_version, protocol_version)` so old records replay correctly even after rule changes.
- **棋譜を読込** — load a saved archive back in, via file picker or pasted text. Replays through the same navigation (← / →, sumi ink, Japanese notation); branching from a past position still works. The embedded version tuple and result are shown; if the archive's rule version doesn't match the running engine, a plain-language warning is shown (replay still proceeds — it just may not reproduce the original outcome exactly). Old bare-kifu files (pre-v0.8.0) still load. Since this reads externally-supplied text, it rejects archives over 500 plies or 512 KB with a friendly message (client-side hygiene limit, not a game rule), and JSON escaping was hardened against control characters in free-text header fields (v0.8.2).

All positions are computed by the Wasm engine at runtime. No hardcoded SFEN data.

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

- **駒をクリック**すると合法手が淡い点で表示される（エンジンが v0.5 ルールを適用）。
- **オフラインモード** — 一人で先後両方を操作。毎ターン両着手を同時確定。
- **オンラインモード** — 同一ルームの 2 名がコミット秘匿→同時開示方式で対戦（秘密情報保護）。
- **← / → ナビ** — 過去局面へ戻ってそこから指し直すと棋譜が分岐。
- **成りダイアログ** — 任意成りが可能な着手で表示。
- **日本語棋譜表記** — ５八金右・７六歩など、曖昧さがある場合のみ区別符（右・左・直・上・引・寄）を付加。
- **デモ局面 / 新局** ボタンで 6 組手デモ局を再生、または初期局面にリセット。
- **棋譜を保存** — 対局中・終局後を問わず、版タプル付きアーカイブ書式（`.kifu`、ダウンロード＋クリップボードコピー）で現在の対局を保存。`(ルール版, プロトコル版)` を埋め込むため、ルール変更後も旧記録を正しく再現できる。
- **棋譜を読込** — 保存したアーカイブをファイル選択または貼り付けで読み込み、既存の棋譜ナビ（← / →・水墨盤・日本語表記）でそのまま鑑賞できる。読み込んだ局面から盤クリックで分岐再指しも可能。刻まれた版タプルと結果を表示し、読み込んだアーカイブのルール版と現行エンジンが食い違う場合は平易な注意文を表示する（再生自体は止めない）。v0.8.0 より前の素の棋譜ファイルも読み込める。外部由来の文字列を扱う機能のため、500組手・512KBを超えるアーカイブは穏当なメッセージで拒否する（ゲームルールではなくクライアント側の安全弁）。自由記述欄への制御文字混入に対する JSON エスケープも強化済み（v0.8.2）。

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
