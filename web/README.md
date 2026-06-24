# web/ — Fukanzen Shogi Web Board / 不完全将棋 Web 盤

## English

A static web board for Fukanzen Shogi. No build step, no framework, no server required.

**Live (production):** [fukanzen-shogi.tokuhira.net](https://fukanzen-shogi.tokuhira.net)  
**Dev:** [fukanzen-shogi.pages.dev](https://fukanzen-shogi.pages.dev)

### Current state

Kifu (game record) replay only. A 6-turn demo game is embedded as pre-computed SFEN data.
The Rust engine is **not yet connected** — resolved positions are hardcoded, not computed at runtime.

### Design boundary

`board.js` consumes SFEN position data and renders the board. The rule engine is not involved at runtime.
When the engine is compiled to Wasm, only the data source switches; the rendering layer stays the same.
This separation ("core and shell") mirrors the Rust workspace design.

### How to run locally

Open `web/index.html` directly in any modern browser. No server needed.

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

不完全将棋の静的 Web 盤。ビルド不要、フレームワーク不要、サーバー不要。

**本番（公開）:** [fukanzen-shogi.tokuhira.net](https://fukanzen-shogi.tokuhira.net)  
**開発:** [fukanzen-shogi.pages.dev](https://fukanzen-shogi.pages.dev)

### 現状

棋譜再生のみ。6 組手分のデモ局を SFEN データとして埋め込み済み。
Rust エンジンは**未接続**——解決後局面は事前計算済みのハードコードデータ。

### 設計の境界（殻と核）

`board.js` は SFEN 局面データを受け取って盤面を描画する。ルールエンジンは実行時に関与しない。
エンジンを Wasm 化する際は「データの供給元をエンジンに切り替えるだけ」で描画層はそのまま再利用できる。
この境界は Rust ワークスペースの「核と殻」設計の延長にある。

### ローカルで開く

`web/index.html` をブラウザで直接開くだけ。サーバー不要。

### デプロイ

```sh
# 初回認証
npx wrangler login

# web/ を Cloudflare Pages へデプロイ
npx wrangler pages deploy
```

設定: リポジトリルートの `wrangler.toml`（`pages_build_output_dir = "web"`）。
