# 不完全将棋 実装指示書 — 盤面解釈を核へ返す（board.js 分割 第〇段：Rust へ返す）

> 対象実行者: Claude Code（Sonnet）
> 前提: 配布 v0.11.1（記録係二段目まで実装済み。CI 門番＝Rust の fmt/clippy/test＋server の typecheck/test が立っている）。関連する現物:
> - `web/board.js`（1531 行）の `parseSfen(sfen)`（114–162、約 50 行）が、SFEN 文字列を自前で解釈して `{ board: Map<"file,rank", {kind, side}>, handS: {kind:count}, handG: {kind:count} }` を返している。`kind` は大文字の駒文字（成りは `+` 前置、例 `P`・`+B`）、`side` は `'s'`/`'g'`。**これは engine が既に持つ SFEN 解釈（`engine::serialize::sfen_to_position`）の JS 再実装＝二重定義**。
> - `parseSfen` の消費者: `renderSvg(pos, overlay)`（`{board, handS, handG}` を分解、`board.get("f,r")` で駒取得）、`handleSvgClick`/`_advanceFromReveal`/入力オーバレイ（`pos.board.get("f,r")`・`getHandPieceAt(pos.handG/handS, …)`）、`renderHandArea`（hand を反復）。呼び出しは render・クリック時（cursor 単位で同一 sfen を複数回）。
> - `engine::serialize`: `sfen_to_position(&str) -> Option<Position>`（既存・テスト済み）、`position_to_sfen(&Position) -> String`（駒→SFEN 文字の変換を内包）。`engine::board::{Board, Hand, Position}`（`Board::iter() -> (Square, Piece)`）。`engine::types::{Piece, PieceKind, Side}`。
> - `engine-wasm`（`src/lib.rs`）: `resolve_ply`・`game_status`・`legal_actions`・`build_archive`・`parse_archive`・`evaluate_terminal`・`max_turns` を露出。JSON 出力は手書き `format!`+`escape_json`（`legal_actions` 等）と `serde_json`（`build_archive` 等）を併用。**`serde_json` は依存済み**。
> - web/ は**ビルドレス静的サイト**（Wasm＋ES モジュールを直接読む。バンドラ無し）。web にテストは無い。CI は Rust と server を守るが web には及ばない。
> 関連文書: board.js 分割の方針相談（本書はその**第〇段＝Rust へ返す**）、`不完全将棋_実装指示書_足場の整備`（CI 門番）、`不完全将棋_バックログ_伏線と未決`。
> 性格: board.js を割る前に、**engine が既に持つ「ゲームの真実」を核へ帰す**。第〇段は SFEN→盤面解釈の一点——JS の自前 `parseSfen` を、engine-wasm の getter へ置換する。**消費者に見える形（`{board:Map, handS, handG}`）は一切変えず、導出だけ差し替える**（blast radius 最小）。これで盤面解釈が 124 テストの側に乗り、二重定義のドリフトが消え、後段（純粋 JS の切り出し・状態を持つ分割）の対象そのものが縮む。**ビルドレスのまま**・**製品挙動は不変**（盤は同一に描画される）。

---

## 0. 目的と範囲

- **作るもの**:
  1. `engine-wasm` に **`position_view(sfen) -> JSON`** getter（描画に必要な構造化された盤面を返す。engine の SFEN 解釈を再利用）。
  2. `web/board.js` の `parseSfen` を **「Wasm 呼び出し＋薄い純粋アダプタ」** に置換（消費者の `{board:Map, handS, handG}` 形は不変、自前 50 行パーサを削除）。
  3. Rust テスト（getter）＋**初の web 純粋 JS テスト**（アダプタ。Wasm 不要）＋最小の web テスト足場。
- **位置づけ**: board.js 分割の**第〇段（Rust へ返す）**。「割る前に、割らなくていいものを核へ帰す」。盤面解釈を核へ寄せ、engine-wasm が描画用の view 構造を返すパターンを確立し、第一/二段の土台にする。
- **作らないもの（＝返さないもの・後段。理由つき）**:
  - **`parseUsi`**: 13 行の自明な USI 文字列分解。ゲーム論理を持たず（USI 形式は固定）ドリフトの危険がほぼゼロ。Wasm 越しに毎回呼ぶ価値が薄い。→ **第一段の純粋 JS util** として切り出しテスト。
  - **`usiToText`**: 既に `notation-wasm`（`ja_notation`）へ委譲済み。JS は前置記号（☗/☖）と糊だけ。返す実体がない。
  - **終局の日本語メッセージ化（`terminalMessageJa`・`formatResult`）**: 終局判定そのものは既に `evaluate_terminal`（engine）にある。JS に残るのは「種別→日本語表示文字列」＝**presentation（ビュー/i18n）**であり、ゲームの真実ではない。→ **盤（JS）に残す**（第一段で純粋 util 化）。
  - **描画・入力・DOM**（`renderSvg`・`handleSvgClick`・入力状態機械）: ピクセルと操作は盤の領分。核へ渡すと Wasm が DOM に密結合して逆に硬くなる。→ **盤に残す**。
  - board.js の構造的分割（第一/二段）。

---

## 1. 境界の原則（返すもの／残すもの）

**核（engine）へ返すのは「ゲームの真実」、盤（JS）に残すのは「その見せ方と入力」。** SFEN が意味する盤面（どのマスに何の駒があるか・持ち駒）はゲームの真実——engine の領分。SVG 座標・水墨の描画・クリック・DOM・日本語表示文字列は見せ方——盤の領分。`position_view` は前者だけを返す。この線を越えて描画まで Rust に持たせない（密結合を避ける）。

---

## 2. engine-wasm: `position_view` getter

```rust
/// SFEN を解釈し、描画に必要な構造化盤面を JSON で返す。
/// engine::serialize::sfen_to_position を再利用する（SFEN 解釈の単一の正本）。
///
/// 返値（成功）:
/// {
///   "board": [ {"file":2,"rank":8,"kind":"R","side":"s"},
///              {"file":8,"rank":8,"kind":"B","side":"s"},
///              {"file":5,"rank":3,"kind":"+P","side":"g"}, ... ],
///   "hand_s": {"P":2,"G":1},
///   "hand_g": {"P":1}
/// }
/// 返値（失敗）: {"error":"bad_sfen"}
#[wasm_bindgen]
pub fn position_view(sfen: &str) -> String;
```

- **実装**: `engine::serialize::sfen_to_position(sfen)` で `Position` を得る（`None` → `{"error":"bad_sfen"}`）。`position.board.iter()` で各 `(Square, Piece)` を、持ち駒を `Hand` から拾って JSON 化。`serde_json`（依存済み）で組み立ててよい。
- **`kind` の規約（JS 消費者に合わせる）**: 駒種を**大文字の SFEN 基本文字**（`P L N S G B R K`）にし、成りは **`+` 前置**（例 `"+P"`・`"+B"`）。**side は文字に埋め込まず** `"side"` に分離（`"s"`/`"g"`）。※ `position_to_sfen` は先後を文字の大小で表すが、ここでは JS の `parseSfen` に合わせ「大文字文字＋別フィールド side」とする。
- **DRY（小さな抽出）**: 駒種→大文字文字の対応は `position_to_sfen` の内部にあるはず。共通化のため `engine::serialize` に `pub fn piece_kind_char(kind: PieceKind) -> char`（大文字を返す）や `pub fn piece_view_kind(piece: &Piece) -> String`（`+` 前置つき）を切り出し、`position_to_sfen` と `position_view` の両方から使う。二度書きを避ける。
- **file/rank の規約**: SFEN の座標に一致させる——**file は 9〜1（SFEN 盤面文字列の左端が file 9）、rank は 1〜9（先頭行が rank 1）**。`parseSfen` と同じ番号付け。`Square` → (file, rank) の写像は engine が持つ（`sfen_to_position`/`position_to_sfen` が round-trip する）ので、それに整合させる。
- 出力する `board` は**駒のあるマスのみ**（空マスは含めない）。

---

## 3. web/board.js: `parseSfen` の置換

`parseSfen` の**戻り値の形は変えない**（`{ board: Map, handS, handG }`）。自前パースを、Wasm 呼び出し＋純粋アダプタに差し替える。

```js
import { /* … */ position_view as wasmPositionView } from './wasm/engine_wasm.js';

// JSON view → 従来の消費者形（Map と持ち駒オブジェクト）。純粋・Wasm 不要・テスト可能。
function positionViewToState(view) {
  const board = new Map();
  for (const sq of view.board) {
    board.set(`${sq.file},${sq.rank}`, { kind: sq.kind, side: sq.side });
  }
  return { board, handS: view.hand_s || {}, handG: view.hand_g || {} };
}

function parseSfen(sfen) {
  return positionViewToState(JSON.parse(wasmPositionView(sfen)));
}
```

- **自前の 50 行 SFEN パーサ（114–162）を削除**。`renderSvg`・`handleSvgClick`・`_advanceFromReveal`・`renderHandArea`・`getHandPieceAt` は**一切変更しない**（`{board:Map, handS, handG}` を受け取り続ける）。
- **任意の微キャッシュ**: `parseSfen` は render・クリックで同一 sfen に対し複数回呼ばれ得る。毎回 Wasm を呼ぶコストが気になるなら、`gameOverCache`/`legalCache` と同じ発想で `positionViewCache = { sfen, state }` の単一エントリ・キャッシュを置く。盤はリアルタイム走行ではない（一手ごとの再描画）ので**必須ではない**——過剰最適化はしない。頭の隅に。

---

## 4. テスト

- **Rust（engine-wasm / engine、CI 門番が守る）**:
  - `position_view` を既知 SFEN で検証。少なくとも: 初期局面（`lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1`）で **file2/rank8 = `{kind:"R",side:"s"}`、file8/rank8 = `{kind:"B",side:"s"}`**（ルール仕様の「飛2八・角8八」に一致）、後手歩が rank3 に 9 枚、持ち駒空。成りを含む局面（`+P` 等）、持ち駒を含む局面、不正 SFEN → `{"error":"bad_sfen"}`。
  - 抽出した `piece_kind_char`/`piece_view_kind` の単体テスト、`position_to_sfen` の既存テストが不変で通ること。
- **純粋 JS（アダプタ、Wasm 不要＝初の web テスト）**: `positionViewToState(sampleJson)` が期待する `{board:Map, handS, handG}` を返す。手書きの view JSON を与えて Map のキー/値・持ち駒を検証。
- **移行時の等価確認（開発時・一度きり）**: 旧 `parseSfen` を `parseSfenLegacy` として一時的に残し、代表 SFEN 群（初期・中盤・成り・持ち駒あり/なし）で `parseSfen(sfen)` と `parseSfenLegacy(sfen)` が deep-equal になることを確認してから legacy を削除する。
- **受け入れ（手動・視覚）**: 盤が従来と**視覚的に同一**に描画される（初期・成りを含む中盤・持ち駒あり）。クリック選択・再生・オンライン・観戦が回帰しない。

---

## 5. 最小の web テスト足場（純粋のみ）

- `web/package.json` を新設し、**vitest** で**純粋 JS**（`positionViewToState`）をテストする（`npm test`）。**Wasm を node で読む足場は本書では組まない**——それは Wasm を要する関数のテストが必要になる第一段へ送る。ここでは Wasm に触れない純粋関数だけを対象にする（ビルドレスの静的サイトは維持。vitest は開発/CI 用で、配信物には影響しない）。
- CI（`.github/workflows/ci.yml`）に**最小の web ジョブ**を足す（`cd web && npm ci && npm test`）。これで CI が初めて web に触れる（純粋部分だけだが、第一/二段で広げる土台）。
- テスト対象を純粋関数に限るため、`positionViewToState` は**副作用なし・Wasm 非依存**に保つ（`parseSfen` から分離しておく＝§3 の形）。

---

## 6. 版の刻み

- **製品挙動は変わらない**（盤は同一に描画され、ルール・プロトコル・棋譜書式も不変）。ゆえに**配布版の bump は必須ではない**（整備・リファクタと同じ扱い）。engine-wasm の Wasm 成果物は再ビルドが要る（`position_view` 追加のため）が、外から見える振る舞いは同じ。
- 節目として区切るならパッチ v0.11.1 → v0.11.2。判断はお任せ（既定は据え置き）。**RULE 0.6・PROTOCOL 4・アーカイブ書式 1 は当然すべて不変。**

---

## 7. 申し送り（後段）

- **第一段（純粋 JS の切り出し＋Wasm-in-node の web テスト足場）**: `parseUsi`・`terminalMessageJa`・`formatResult`・定数/対応表・`renderSvg`（`pos`+`overlay`→文字列の純粋部分）を ES モジュールへ切り出し、web テストを広げる。Wasm を要する糊（`usiToText` 等）のテストのため、node で Wasm を読む足場をここで組む。
- **第二段（状態を持つ分割）**: model / view / controller を、大域変数から小さな状態モジュールへ移して分ける。第〇段・第一段でテスト網が揃ってから。
- バックログの「board.js 分割」項目に、第〇段 済・第一段/第二段 待ち、と状態を刻む。

---

## 8. 不変の原則（本実装が守るもの）

1. **ロジックは核へ、ピクセルは盤に**: SFEN の意味（ゲームの真実）は engine が返し、座標・描画・DOM・表示文字列は盤に残す。
2. **消費者の形を変えず、導出だけ差し替える**: `parseSfen` の戻り値 `{board:Map, handS, handG}` は不変。renderSvg もクリック処理も無変更（blast radius 最小）。
3. **二重定義を消す**: SFEN 解釈を `sfen_to_position` に一本化し、JS の並行パーサを削除。ドリフトの温床を断つ。盤面解釈が 124 テストの側に乗る。
4. **ビルドレス維持・挙動不変**: バンドラを導入せず ES モジュールのまま。盤は同一に描画される。
5. **段階的・最小**: 第〇段は getter 一つと薄いアダプタだけ。返すべきものだけ返す（自明な `parseUsi`・presentation の `terminalMessageJa` は返さない）。

---

*board.js 分割の第〇段——盤面解釈を核へ返す。engine が既に持つ SFEN 解釈の JS 重複（自前 `parseSfen`）を、engine-wasm の `position_view` getter へ置換する。消費者に見える形（`{board:Map, handS, handG}`）は変えず、導出だけを差し替える（renderSvg・クリック処理は無変更）。盤面解釈が 124 テストの側に乗り、二重定義のドリフトが消え、割る対象そのものが縮む。初の web 純粋テストと最小の web CI ジョブも据える。ビルドレス維持、製品挙動は不変、版 bump は任意。ロジックは核へ、ピクセルは盤に。*
