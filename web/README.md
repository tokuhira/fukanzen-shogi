# web/ — Fukanzen Shogi Web Board / 不完全将棋 Web 盤

## English

A static web board for Fukanzen Shogi. No framework, no server required for production.

**Live (production):** [fukanzen-shogi.tokuhira.net](https://fukanzen-shogi.tokuhira.net)  
**Dev:** [fukanzen-shogi.pages.dev](https://fukanzen-shogi.pages.dev)

### Current state

Interactive hotsheet board. One person plays both sides with mouse/click.
The `engine` crate is compiled to WebAssembly (`web/wasm/`) and called from `board.js`.

- **Click a piece** to see legal moves as subtle ink dots on the board (v0.5 rules enforced by engine).
- **Sente first, then Gote** — click a piece for each side to build the paired move.
- **Resolve** (button or keyboard) — calls `resolve_ply`; the move is appended to the kifu.
- **Navigate** with ← / → buttons or arrow keys; revisiting any past position and playing from there branches the kifu.
- **Promotion dialog** — appears on moves that can optionally promote.
- **デモ局面 / 新局** buttons load the built-in 6-turn demo or reset to a blank board.

All positions are computed by the Wasm engine at runtime. No hardcoded SFEN data.

### Design boundary

`board.js` imports three Wasm functions: `resolve_ply` (resolve a paired move),
`game_status` (check for forced termination), and `legal_actions` (enumerate legal moves
for one side). The engine is the sole source of rule truth; the JS layer handles
only UI state and rendering. The rendering layer (`parseSfen`, `renderSvg`, `renderHandArea`)
is unchanged from the kifu-replay prototype.

### How to run locally

WebAssembly requires HTTP (not `file://`). Use any local HTTP server:

```sh
python3 -m http.server 8080 --directory web
# then open http://localhost:8080
```

### Rebuild Wasm

```sh
wasm-pack build engine-wasm --target web --out-dir ../web/wasm --no-typescript
rm -f web/wasm/.gitignore   # wasm-pack generates this; remove to allow committing artifacts
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

不完全将棋の静的 Web 盤。フレームワーク不要、本番はサーバー不要。

**本番（公開）:** [fukanzen-shogi.tokuhira.net](https://fukanzen-shogi.tokuhira.net)  
**開発:** [fukanzen-shogi.pages.dev](https://fukanzen-shogi.pages.dev)

### 現状

棋譜再生。6 組手分のデモ局を Wasm エンジンがブラウザ上でリアルタイム計算。
`engine` クレートを WebAssembly にコンパイルし（`web/wasm/`）、`board.js` から呼び出す。
解決後局面はエンジンが計算するためハードコード不要。

### 設計の境界（殻と核）

`board.js` は Wasm エンジンに初期 SFEN と各組手の USI 着手を渡し、
返ってきた SFEN を描画する。描画層（`parseSfen`・`renderSvg`）は無改変。
棋譜を追加するには `TURNS` に着手列を追加するだけで、局面の手計算は不要。

### ローカルで動かす

Wasm は `file://` では動かないので HTTP サーバーが必要：

```sh
python3 -m http.server 8080 --directory web
# ブラウザで http://localhost:8080 を開く
```

### Wasm 再ビルド

```sh
wasm-pack build engine-wasm --target web --out-dir ../web/wasm --no-typescript
rm -f web/wasm/.gitignore   # wasm-pack が生成する .gitignore を削除（コミットできるようにするため）
```

### デプロイ

```sh
# 初回認証
npx wrangler login

# web/ を Cloudflare Pages へデプロイ
npx wrangler pages deploy
```

設定: リポジトリルートの `wrangler.toml`（`pages_build_output_dir = "web"`）。
