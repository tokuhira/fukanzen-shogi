# 不完全将棋 実装指示書 — 第二段b：着手組み立ての計算を純粋化する（`move-input.js`・案A）

> 対象実行者: Claude Code（Sonnet 5 または Haiku 4.5）
> 前提: 配布 v0.11.2 / web `?v=`0.11.5（board.js 分割 第二段a まで着地。web テスト 30 件・Wasm-in-node 足場・注入パターン〔純粋モジュール＋board.js の薄いラッパ〕・`game-record.js` が据わっている）。
> 関連する現物（すべて実地で確認済み）:
> - 入力島の大域可変状態は board.js の `inputStep`・`pendingSente`・`pendingGote`・`selectedFrom`・`legalTargets`・`promotionPending`。**本書はこれらを移動しない**（第二段a と同じ規律——状態は据え置き、抜くのは計算だけ）。
> - 抜き出す純粋計算は、選択・確定ロジックの中に埋もれている: 合法手を「盤上の from から」で絞る（`selectBoardPiece` の filter）・「持ち駒の打ちで」絞る（`selectHandPiece` の filter）・到達点マップを組む（`buildTargetMap`、435–443）・到達点クリックから**成り不成を判定して次アクションを決める**（`selectTarget` の `hasPromote && hasNoPromote` 分岐、467–480）。いずれも入力に対し出力が決まり、DOM にも可変状態にも触れない。
> - 混ざる不純（board.js に残す）: 状態代入（`selectedFrom = ...`・`legalTargets = ...`・`promotionPending = ...`・`inputStep = ...`）、DOM（`showPromotionUI`/`hidePromotionUI`）、I/O とオンライン分岐（`confirmMove`・`commitMoveOnline`）、Wasm＋cache（`getLegalMovesForSide`）、座標→マス（`getBoardSquare`/`getHandPieceAt`、geometry 依存）。
> - `parseUsi`（`usi.js`）の返り: `{usi, isDrop, kind?, from?, to:[file,rank], promote}`。合法手は `getLegalMovesForSide` が `JSON.parse(legal_actions(...)).map(parseUsi)` で得た**パース済み配列**。純粋関数はこのパース済み配列を受ける（Wasm も parseUsi も呼ばない）。
> - **実地検証済み**（node で実 Wasm）: 8八角が3三へ入る局面で `8h3c`（不成）と `8h3c+`（成）が両方 options に入り、成り判定が `promptPromotion` を返す。到達点クリックは `confirm`、非到達点は `deselect`。フィルタ・targetMap も正しく動く。
> 関連文書: `不完全将棋_実装指示書_棋譜コアの遷移を純粋化_board分割第二段a`、`不完全将棋_実装指示書_Wasm足場と棋譜の糊_board分割第一段b`、`不完全将棋_バックログ_伏線と未決`。
> 性格: 第二段b（案A・最小）は**「着手組み立ての純粋計算だけを抜き、実 Wasm でテストして固める」**。最も価値ある純粋部分＝**合法手フィルタと成り不成の判定**（将棋のルール的に間違えると実害）をテスト網に載せる。選択の状態機械（`handleSvgClick` のトグル解除・別駒切り替え）と `confirmMove` のオンライン分岐は**触らず board.js に残す**（次段・view 分離で扱う方が自然）。状態変数は据え置き。Rust に触れず Wasm 再ビルドなし。製品挙動は不変。行番号は v0.11.5 の board.js 基準の目安——**関数名で位置を特定**。

---

## 0. 目的と範囲

- **作るもの**:
  1. `web/move-input.js` — 着手組み立ての純粋計算。パース済み合法手配列を受け、状態にも DOM にも Wasm にも触れず、値を返す。
  2. `web/test/move-input.test.js` — Wasm-in-node 足場で実 `legal_actions`＋`parseUsi` から合法手を作り、純粋計算を検証（特に成り不成の分岐）。
- **位置づけ**: board.js 分割の**第二段b（案A）**。入力島の純粋な芯（フィルタ・targetMap・成り判定）を抜いてテストで固める。状態と状態機械は据え置き。
- **作らないもの（＝理由つき）**:
  - **入力島の状態変数の移動**（`inputStep`/`selectedFrom`/…）: 据え置き。移すのは計算のみ。
  - **選択の状態機械の純粋化**（`handleSvgClick` の 110 行——トグル解除・別駒切り替え・deselect）: `cursor`/`phase`/`render()` と絡む。view 分離（後段）で「クリック→意図」として扱う方が自然。今 reduce 化すると二度手間。**据え置き**。
  - **`confirmMove` のオンライン/ホットシート分岐**: `commitMoveOnline`（I/O）・`pendingSente/Gote` 代入を含む。純粋化の旨味が薄い。**据え置き**。
  - **`getLegalMovesForSide` の cache**（`legalCache`）: Wasm＋可変 cache。board.js に残す（純粋関数へは cache 済み配列を渡す）。

---

## 1. `web/move-input.js`（純粋・状態も DOM も Wasm も触れない）

パース済み合法手 `move = {usi, isDrop, kind?, from?, to:[f,r], promote}` の配列を受ける。

```js
// 着手組み立ての純粋計算。パース済み合法手配列（parseUsi 済み）を受け、状態・DOM・Wasm に
// 触れず値を返す。board.js 分割 第二段b（案A）。
// move = { usi, isDrop, kind?, from?:[f,r], to:[f,r], promote }

// 盤上マス (file,rank) から動ける手だけに絞る。
export function movesFromSquare(moves, file, rank) {
  return moves.filter(m => !m.isDrop && m.from[0] === file && m.from[1] === rank);
}

// 持ち駒 kind の打ちだけに絞る（kind は大文字化して比較）。
export function dropsOfKind(moves, kind) {
  return moves.filter(m => m.isDrop && m.kind === kind.toUpperCase());
}

// 手の集合を「到達点 → options」のマップに畳む。options[i] = { usi, promote }。
export function buildTargetMap(moves) {
  const map = new Map();
  for (const m of moves) {
    const key = `${m.to[0]},${m.to[1]}`;
    if (!map.has(key)) map.set(key, { options: [] });
    map.get(key).options.push({ usi: m.usi, promote: m.promote });
  }
  return map;
}

// 到達点クリックから次アクションを決める（成り不成の判定）。状態は変えず、意図だけ返す。
//   到達点でない        → { kind: 'deselect' }
//   成・不成が両立       → { kind: 'promptPromotion', options, toSquare:[f,r] }
//   一方のみ            → { kind: 'confirm', usi }
export function resolveTarget(targetMap, file, rank) {
  const key = `${file},${rank}`;
  if (!targetMap.has(key)) return { kind: 'deselect' };
  const { options } = targetMap.get(key);
  const hasPromote   = options.some(o =>  o.promote);
  const hasNoPromote = options.some(o => !o.promote);
  if (hasPromote && hasNoPromote) return { kind: 'promptPromotion', options, toSquare: [file, rank] };
  return { kind: 'confirm', usi: options[0].usi };
}
```

- すべて純粋（引数のみに依存、Wasm も `parseUsi` も呼ばない、状態も DOM も触らない）。`buildTargetMap`・`resolveTarget` は board.js の現行実装から**ロジック無変更で**移す。`movesFromSquare`/`dropsOfKind` は `selectBoardPiece`/`selectHandPiece` の filter 式をそのまま関数化。

## 2. board.js 側の書き換え（計算だけ委譲・状態代入と DOM/I/O は残す）

import 追加:

```js
import { movesFromSquare, dropsOfKind, buildTargetMap, resolveTarget } from './move-input.js';
```

board.js から `buildTargetMap`（435–443）の定義を削除（`move-input.js` へ移動済み）。`selectBoardPiece`/`selectHandPiece`/`selectTarget` を、純粋計算を呼んで状態へ代入する形に：

```js
function selectBoardPiece(file, rank) {
  if (!inputStep) inputStep = 'sente';
  const side  = inputStep === 'gote' ? 'gote' : 'sente';
  const moves = movesFromSquare(getLegalMovesForSide(side), file, rank);
  activateMoves(moves, { board: [file, rank] });
}

function selectHandPiece(kind) {
  if (!inputStep) inputStep = 'sente';
  const side  = inputStep === 'gote' ? 'gote' : 'sente';
  const moves = dropsOfKind(getLegalMovesForSide(side), kind);
  activateMoves(moves, { hand: kind });
}

function selectTarget(file, rank) {
  const action = resolveTarget(legalTargets, file, rank);
  if (action.kind === 'deselect') {
    selectedFrom = null; legalTargets = null;
  } else if (action.kind === 'promptPromotion') {
    promotionPending = { options: action.options, toSquare: action.toSquare };
    showPromotionUI();
  } else { // 'confirm'
    confirmMove(action.usi);
    render();
  }
}
```

- `activateMoves`（`selectedFrom`/`legalTargets` へ代入、`buildTargetMap` を呼ぶ）は board.js に残す。内部で `move-input.js` の `buildTargetMap` を使う（import 済み）:
```js
function activateMoves(moves, from) {
  if (!moves.length) { selectedFrom = null; legalTargets = null; return; }
  selectedFrom = from;
  legalTargets = buildTargetMap(moves);
}
```
- **挙動保存**: `selectTarget` の三分岐（deselect / promptPromotion＋`showPromotionUI` / confirm＋`render`）は元と厳密に同じ。`options[0].usi` を confirm に渡す点も不変。`showPromotionUI`/`hidePromotionUI`/`confirmMove`/`commitMoveOnline` は board.js のまま無変更。
- `handleSvgClick`（651–）は**無変更**（トグル解除・別駒切り替えの状態機械は据え置き）。ただし `handleSvgClick` 内で `legalTargets.has(key)` を見てから `selectTarget` を呼ぶ流れは、`selectTarget` 冒頭の `resolveTarget` が同じ判定を含むため二重に見えるが、**元の構造を保存**するため handleSvgClick 側は触らない（deselect 分岐が両方にあっても挙動は同じ）。

## 3. テスト `web/test/move-input.test.js`

Wasm-in-node 足場で実 `legal_actions` から合法手を作り（`parseUsi` でパース）、純粋計算を検証。**成り不成の分岐が主眼**。

```js
import { describe, it, expect, beforeAll } from "vitest";
import { movesFromSquare, dropsOfKind, buildTargetMap, resolveTarget } from "../move-input.js";
import { parseUsi } from "../usi.js";
import { loadEngine } from "./wasm-loader.js";

// 8八角が3三へ入れる局面（8h3c と 8h3c+ が両立）。実地で確認済み。
const SFEN = "lnsgkgsnl/1r5b1/pppppp1pp/6p2/9/2P6/PP1PPPPPP/1B5R1/LNSGKGSNL b - 5";
let senteMoves;
beforeAll(async () => {
  const engine = await loadEngine();
  senteMoves = JSON.parse(engine.legal_actions(SFEN, "sente")).map(parseUsi);
});

describe("move-input（着手組み立ての純粋計算）", () => {
  it("movesFromSquare は盤上 from の手だけに絞る", () => {
    const ms = movesFromSquare(senteMoves, 8, 8); // 8八角
    expect(ms.length).toBeGreaterThan(0);
    expect(ms.every(m => !m.isDrop && m.from[0] === 8 && m.from[1] === 8)).toBe(true);
  });

  it("dropsOfKind は打ちが無ければ空（この局面は持ち駒なし）", () => {
    expect(dropsOfKind(senteMoves, "P")).toEqual([]);
  });

  it("buildTargetMap は到達点ごとに options を畳む", () => {
    const tm = buildTargetMap(movesFromSquare(senteMoves, 8, 8));
    const entry = tm.get("3,3"); // 3三へ
    expect(entry).toBeTruthy();
    expect(entry.options.some(o => o.promote)).toBe(true);
    expect(entry.options.some(o => !o.promote)).toBe(true);
  });

  it("resolveTarget: 成不成が両立→promptPromotion", () => {
    const tm = buildTargetMap(movesFromSquare(senteMoves, 8, 8));
    const a = resolveTarget(tm, 3, 3);
    expect(a.kind).toBe("promptPromotion");
    expect(a.toSquare).toEqual([3, 3]);
    expect(a.options.length).toBe(2);
  });

  it("resolveTarget: 一方のみ→confirm（usi を返す）", () => {
    // 2七歩を2六へ（不成のみ）
    const tm = buildTargetMap(movesFromSquare(senteMoves, 2, 7));
    const a = resolveTarget(tm, 2, 6);
    expect(a.kind).toBe("confirm");
    expect(a.usi).toBe("2g2f");
  });

  it("resolveTarget: 到達点でない→deselect", () => {
    const tm = buildTargetMap(movesFromSquare(senteMoves, 2, 7));
    expect(resolveTarget(tm, 5, 5)).toEqual({ kind: "deselect" });
  });
});
```

## 4. 受け入れ

- `cd web && npm test` が緑（既存 30 件＋新規 `move-input` 6 件、warn なし）。
- ブラウザで従来通り: 盤上の駒選択→到達点の墨点表示→クリックで確定、持ち駒選択→打ち先表示→確定、**成り不成の両立局面で成り選択 UI が出る**（片方のみなら即確定）、到達点以外クリックで解除、オンライン/ホットシート両モードでの確定。
- **特に確認**: 成り不成が両立する手（角・飛・歩香桂銀の敵陣出入り）で `showPromotionUI` が出ること、片方のみの手で即 `confirmMove` されること（`options[0].usi`）。

## 5. 版の刻み

- **製品挙動は不変・Rust 非関与・Wasm 再ビルドなし**。第二段a と同じ扱い: 配布版据え置き **v0.11.2**、web の `?v=`（`web/package.json`・`web/index.html`）を **0.11.6** へ独立に前進（board.js が `move-input.js` を新規 import するためキャッシュ更新）。**RULE 0.6・PROTOCOL 4・アーカイブ書式 1 不変**。

## 6. 申し送り（次段へ）

- 入力島の**純粋計算**（フィルタ・targetMap・成り判定）が固定された。残るは**選択の状態機械**（`handleSvgClick` のトグル解除・別駒切り替え・deselect）と `confirmMove` のオンライン分岐——これらは view 分離（`render`/クリック→意図）と一緒に扱うのが自然。第二段a の `setRecord`/`currentRecord` と本段の純粋計算が、その入口の足場になる。
- view 分離に入るとき、`computeInputOverlay`（selectedFrom/legalTargets → 墨点・強調の overlay 計算、684–）も純粋化候補（`board-view.js` の renderSvg が受ける overlay を作る部分）。golden snapshot への局面追加（第一段a の申し送り）もそこで。

---

## 7. 不変の原則（本実装が守るもの）

1. **状態は動かさず、計算だけ純粋化する**（案A）: `inputStep`/`selectedFrom`/`legalTargets`/`promotionPending` は board.js 据え置き。抜くのはフィルタ・targetMap・成り判定のみ。
2. **純粋**: `move-input.js` は引数のみに依存し、Wasm も `parseUsi` も呼ばず、状態も DOM も触らない。パース済み合法手を受け、値（絞った配列・マップ・アクション意図）を返す。
3. **挙動保存**: `selectTarget` の三分岐（deselect/promptPromotion＋showPromotionUI/confirm＋render）と `options[0].usi` の確定、`handleSvgClick` の状態機械は元と厳密に同じ。
4. **状態機械と I/O は次段へ**: `handleSvgClick` のトグル・`confirmMove` のオンライン分岐は触らない（view 分離で扱う）。過ぎたるは及ばざる。
5. **Rust に触れず Wasm を再ビルドしない**: 純粋 JS ＋テストのみ。配布版据え置き、web `?v=` のみ前進。

---

*第二段b（案A）——着手組み立ての計算を純粋化する。入力島は純粋計算・状態機械・DOM/I/O が混ざる。案A は最も価値ある純粋部分＝合法手フィルタと成り不成の判定だけを `move-input.js` へ抜き、実 Wasm で固める（8八角が3三へ入る手で成・不成が両立し promptPromotion が返ること、片方のみなら confirm、非到達点なら deselect——地面で確認済み）。状態変数（`inputStep`/`selectedFrom`…）は据え置き、board.js には純粋計算を呼んで状態へ代入する薄い `selectBoardPiece`/`selectHandPiece`/`selectTarget` を残す。選択の状態機械（handleSvgClick のトグル）と confirmMove のオンライン分岐は触らず、view 分離と一緒に扱う次段へ送る。ルールの核心（成り判定）を錠し、状態と I/O には手を触れない。*
