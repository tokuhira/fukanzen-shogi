# 不完全将棋 実装指示書 — 第一段b：Wasm-in-node のテスト足場と、棋譜の糊を核へ寄せる

> 対象実行者: Claude Code（Sonnet 5 または Haiku 4.5）
> 前提: 配布 v0.11.2 / web `?v=`0.11.3（board.js 分割 第一段a＝純粋の収穫、まで着地。`usi.js`・`result-view.js`・`geometry.js`・`board-view.js` と web テスト 21 件・golden snapshot が据わっている。web の vitest（v4.1.10、`type:module`）は `npm test`＝`vitest run` で回り、CI の web ジョブが自動で拾う）。
> 関連する現物（すべて実地で確認済み）:
> - **三つの Wasm は別々のディレクトリに**置かれている: `web/wasm/engine_wasm.js`（＋`engine_wasm_bg.wasm`）、`web/notation-wasm/notation_wasm.js`、`web/protocol-wasm/protocol_wasm.js`。それぞれ default export が非同期 init（wasm-bindgen 生成の `__wbg_init`）。
> - init は `module_or_path` を受け、**`{ module_or_path: <bytes> }` の形（Object.prototype を持つオブジェクト）で渡すと fetch 分岐を通らず `WebAssembly.instantiate(bytes)` に直行する**。node で `fs.readFileSync` したバイト列をこの形で渡せば、ブラウザ側の既定 fetch 経路（`new URL(..., import.meta.url)`）を一切変えずに読める。**bytes を裸で渡すと動くが「deprecated parameters」warn が出る**ので、必ずオブジェクト形で渡す。
> - Wasm を跨ぐ薄い糊: `board.js` の `usiToText(usi, sfen, side)`（52–56）＝ `prefix('☗'/'☖') ＋ ja_notation(usi, side, legal_actions(sfen, side), sfen)`。**engine-wasm（`legal_actions`）と notation-wasm（`ja_notation`）の両方を跨ぐ**。呼び出し元は棋譜テキストの遅延生成（136–137・184–185）。
> - 正しいシグネチャ（型定義で確認）: `legal_actions(sfen: string, side: string): string`、`ja_notation(usi: string, side: string, legal_json: string, sfen: string): string`、`position_view(sfen): string`、`max_turns(): number`。
> 関連文書: `不完全将棋_実装指示書_純粋の収穫_board分割第一段a`（親。§0 で第一段b を「Wasm を要するユニットが現れたとき組む」と申し送っていた。本書がそれ）、`不完全将棋_バックログ_伏線と未決`。
> 性格: 第一段a が「Wasm を要さない純粋部分の収穫」だったのに対し、第一段b は**「node で Wasm を読むテスト足場を初めて据え、Wasm を跨ぐ糊をテスト網に載せる」**。足場は既存のブラウザ経路（fetch）に触れず、**ビルドレスを維持**。Rust には触れず Wasm も再ビルドしない。製品挙動は不変。行番号は v0.11.3 の board.js 基準の目安——**関数名で位置を特定**すること。

---

## 0. 目的と範囲

- **作るもの**:
  1. `web/test/wasm-loader.js` — node で三つの Wasm を `fs.readFileSync` → `init({module_or_path: bytes})` で読む共有ヘルパ（テスト専用。本番 board.js は読まない）。
  2. `web/notation-view.js` — `usiToText` を board.js から薄いモジュールへ寄せる。**Wasm 関数は引数で注入**し、モジュール自身は Wasm に直接 import 依存しない（テストから注入でき、board.js からは実 Wasm 関数を渡す）。
  3. `web/test/notation-view.test.js` — 足場で実 Wasm を読み、`usiToText` が二つの Wasm を跨いで正しい日本語棋譜を生むことを検証。
- **位置づけ**: board.js 分割の**第一段b**。第一段a の申し送り通り、「Wasm を要するユニットが現れたとき足場を組む」の実行。ここで初めて web テストが実 Wasm を node で読めるようになり、第二段（状態を持つ分割）で controller ロジックを Wasm 越しに検証する道が開く。
- **作らないもの（＝理由つき）**:
  - **board.js の本番コードの Wasm 読み込み方法の変更**: 足場は**テスト専用**。board.js は従来通り `import init, {...} from './wasm/engine_wasm.js'` のまま（ブラウザは fetch 経路で読む）。足場は同じ生成コードを別の入口（bytes 注入）から叩くだけ。
  - **protocol-wasm を使うテスト**: 現時点で Wasm を跨ぐ糊は `usiToText` のみ。`version_tuple`（protocol）を要するテスト対象はまだ無い。足場ヘルパは三つとも読めるように書くが、テストは engine＋notation のみ。**必要が生じたら足す**。
  - **状態を持つ分割**（model/view/controller）: 第二段。本書は足場と、Wasm を跨ぐ純粋な糊一つを載せるまで。

---

## 1. 足場ヘルパ `web/test/wasm-loader.js`

node で wasm-bindgen 生成モジュールを、ブラウザの fetch 経路に触れず読む。**`{ module_or_path: bytes }` 形で渡すのが要**（裸の bytes は warn が出る）。

```js
// node（vitest）で wasm-bindgen 生成モジュールを読むテスト専用ヘルパ。
// 本番の board.js は従来通りブラウザの fetch 経路で読む——ここはテストだけの入口。
// init に { module_or_path: bytes } を渡すと fetch 分岐を通らず WebAssembly.instantiate
// に直行する（bytes を裸で渡すと deprecated 警告が出るのでオブジェクト形で渡す）。

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

async function loadWasm(dir, base) {
  const bytes = readFileSync(
    fileURLToPath(new URL(`../${dir}/${base}_bg.wasm`, import.meta.url))
  );
  const mod = await import(`../${dir}/${base}.js`);
  await mod.default({ module_or_path: bytes });
  return mod;
}

// 遅延ロード（呼ばれたテストだけが実 Wasm を読む）。
export const loadEngine   = () => loadWasm("wasm",          "engine_wasm");
export const loadNotation = () => loadWasm("notation-wasm", "notation_wasm");
export const loadProtocol = () => loadWasm("protocol-wasm", "protocol_wasm");
```

- パスは `web/test/` から見た相対（`../wasm/...` 等）。`import.meta.url` 基準なので実行ディレクトリに依存しない。
- **実地確認済み**: engine を読んで `max_turns()===500`・`position_view(初期SFEN)` が 40 駒、notation と跨いで `ja_notation('7g7f','sente',legal,sfen)==='７六歩'`、初手合法手 30。

## 2. `web/notation-view.js`（糊を薄いモジュールへ寄せる・Wasm は注入）

`usiToText` を board.js から移す。ただし**Wasm 関数（`legal_actions`・`ja_notation`）はモジュール内で import せず、引数で受け取る**。こうするとモジュール自体は純粋な糊のまま——テストからは実 Wasm を注入、board.js からは実 Wasm 関数を渡す。

```js
// 着手（USI）を日本語棋譜テキストへ。Wasm 関数（legalActions・jaNotation）は
// 呼び出し側から注入する（本モジュールは Wasm に直接依存しない）。board.js 分割 第一段b。

// legalActions(sfen, side) -> legal_json, jaNotation(usi, side, legal_json, sfen) -> text
export function usiToText(usi, sfen, side, legalActions, jaNotation) {
  const prefix    = side === "sente" ? "☗" : "☖";
  const legalJson = legalActions(sfen, side);
  return `${prefix}${jaNotation(usi, side, legalJson, sfen)}`;
}
```

### board.js 側の書き換え

- board.js から `usiToText` の定義（52–56）を削除し、`import { usiToText as usiToTextPure } from './notation-view.js';` を追加。
- board.js に**薄いラッパ**を置き、既存呼び出し（136–137・184–185）のシグネチャ `usiToText(usi, sfen, side)` を不変に保つ。実 Wasm 関数を注入する:

```js
// 実 Wasm 関数を綴じ込んだ board.js ローカルの呼び出し口（既存の呼び出し形を保つ）。
function usiToText(usi, sfen, side) {
  return usiToTextPure(usi, sfen, side, wasmLegalActions, wasmJaNotation);
}
```

- これにより 136–137・184–185 の呼び出しは**無変更**。`wasmLegalActions`・`wasmJaNotation` は既に board.js が import 済み（3・14 行）。
- 439 行の `wasmLegalActions(sfen, side)` 直接使用（`legalCache`）は本書の対象外・無変更。

> 設計の含意: `usiToText` の「純粋な糊」部分（prefix 付けと二つの Wasm 呼び出しの合成）が `notation-view.js` に分離され、Wasm 依存が注入点に集約される。第二段で controller を割るとき、この注入パターン（Wasm を引数で受ける純粋モジュール＋board.js の薄いラッパ）が雛形になる。

## 3. テスト `web/test/notation-view.test.js`

足場で engine＋notation を読み、実 Wasm を注入して `usiToText` を検証（`position-view.test.js` と同じ vitest の流儀）。

```js
import { describe, it, expect, beforeAll } from "vitest";
import { usiToText } from "../notation-view.js";
import { loadEngine, loadNotation } from "./wasm-loader.js";

const INITIAL_SFEN =
  "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1";

let legalActions, jaNotation;
beforeAll(async () => {
  const engine   = await loadEngine();
  const notation = await loadNotation();
  legalActions = engine.legal_actions;
  jaNotation   = notation.ja_notation;
});

describe("usiToText（Wasm を跨ぐ糊）", () => {
  it("先手の 7g7f は ☗７六歩", () => {
    expect(usiToText("7g7f", INITIAL_SFEN, "sente", legalActions, jaNotation))
      .toBe("☗７六歩");
  });
  it("後手の接頭は ☖", () => {
    const t = usiToText("3c3d", INITIAL_SFEN, "gote", legalActions, jaNotation);
    expect(t.startsWith("☖")).toBe(true);
  });
});
```

- 検証の主眼は**二つの Wasm を跨いだ合成が正しいこと**と**接頭符号（☗/☖）**。`ja_notation` 自身の網羅は Rust（notation クレート）側の責務なので、ここは糊の検証に絞る（過ぎたるは及ばざる）。
- 後手の具体的な字（`３四歩` 等）は notation の内部規約に委ね、テストは接頭と非空に留める（Rust テストと重複しない）。

## 4. 受け入れ

- `cd web && npm test` が緑（既存 21 件＋新規 `notation-view` 2 件）。**warn（deprecated parameters）が出ないこと**（`{module_or_path:bytes}` 形の確認）。
- ブラウザで盤・棋譜・オンライン・観戦が従来通り（board.js の `usiToText` ラッパ経由で棋譜テキストが従来と同一に出る——特に棋譜再生時の遅延生成 136–137、着手確定時 184–185）。
- 足場は**テスト専用**で、board.js の Wasm import・読み込み経路は無変更（ブラウザは fetch 経路のまま）。

## 5. 版の刻み

- **製品挙動は不変・Rust 非関与・Wasm 再ビルドなし**。整備＋薄い糊の移動なので、第一段a と同じ扱い: 配布版（Cargo ワークスペース）は**据え置き v0.11.2**、web 資産に変更があるので `web/package.json` と `web/index.html` の `?v=` を **0.11.4** へ独立に進める（board.js が notation-view.js を新規 import するため、キャッシュを確実に更新する）。**RULE 0.6・PROTOCOL 4・アーカイブ書式 1 不変**。

## 6. 申し送り（第二段へ）

- **足場が据わった**ので、第二段（状態を持つ分割）で controller ロジックを実 Wasm 越しに node で検証できる。`notation-view.js` の「Wasm を引数注入する純粋モジュール＋board.js の薄いラッパ」を雛形に、`resolve_ply`・`evaluate_terminal` を要する遷移ロジックを同じ形で割り出せる。
- protocol-wasm を跨ぐ糊（`version_tuple` 依存）が現れたら、`loadProtocol` は既に足場にあるので即テストへ載せられる。
- golden snapshot（board-view）に局面を足す件（第一段a の申し送り）は第二段の入口で。

---

## 7. 不変の原則（本実装が守るもの）

1. **足場はテスト専用・本番の経路に触れない**: node は `{module_or_path:bytes}` 注入で読み、ブラウザは従来の fetch 経路のまま。ビルドレス維持。
2. **Wasm 依存は注入点に集約**: 純粋な糊モジュール（`notation-view.js`）は Wasm を import せず引数で受ける。board.js の薄いラッパが実 Wasm を綴じ込み、既存呼び出し形を保つ。
3. **糊の検証に絞る**: `ja_notation` の網羅は Rust の責務。web テストは二つの Wasm を跨ぐ合成と接頭符号だけを見る（重複しない）。
4. **Rust に触れず Wasm を再ビルドしない**: 純粋 JS ＋テスト足場のみ。配布版据え置き、web `?v=` のみ前進。
5. **必要が生じたら足す**: protocol の足場口は用意するが、要るテストが無いうちは載せない。

---

*第一段b——node で Wasm を読む足場を初めて据える。`{module_or_path:bytes}` を注入すればブラウザの fetch 経路に触れずビルドレスのまま実 Wasm を読める、と地面で確かめた（`7g7f→☗７六歩` が二つの Wasm を跨いで通る）。Wasm を跨ぐ薄い糊 `usiToText` を、Wasm を引数注入する純粋モジュール `notation-view.js` へ寄せ、board.js には既存呼び出し形を保つ薄いラッパを残す——この注入パターンが第二段で状態を割るときの雛形になる。足場はテストだけの入口、本番の経路は不変。糊の検証に絞り、Rust の責務とは重複しない。*
