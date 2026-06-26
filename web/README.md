# web/ — Fukanzen Shogi Web Board / 不完全将棋 Web 盤

## English

A static web board for Fukanzen Shogi. No framework, no server required for production.

**Live (production):** [fukanzen-shogi.tokuhira.net](https://fukanzen-shogi.tokuhira.net)  
**Dev:** [fukanzen-shogi.pages.dev](https://fukanzen-shogi.pages.dev)

### Current state

Kifu (game record) replay. A 6-turn demo game is driven by the Wasm engine at runtime.
The `engine` crate is compiled to WebAssembly (`web/wasm/`) and called from `board.js`.
Resolved positions are computed by the engine — no hardcoded SFEN positions.

### Design boundary

`board.js` imports the Wasm engine, feeds it the initial SFEN and each turn's USI moves,
and renders the returned SFEN positions. The rendering layer (`parseSfen`, `renderSvg`) is unchanged.
Adding new game records requires only adding entries to `TURNS` — no manual SFEN computation.

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
