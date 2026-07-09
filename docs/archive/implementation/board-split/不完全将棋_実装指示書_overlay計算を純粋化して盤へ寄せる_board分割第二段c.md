# 不完全将棋 実装指示書 — 第二段c：overlay 計算を純粋化して盤へ寄せる（`board-view.js`）

> 対象実行者: Claude Code（Sonnet 5 または Haiku 4.5）
> 前提: 配布 v0.11.2 / web `?v=`0.11.6（board.js 分割 第二段b まで着地。web テスト 36 件・Wasm-in-node 足場・`game-record.js`・`move-input.js`・`board-view.js`〔`renderSvg`＋geometry〕が据わっている）。
> 関連する現物（すべて実地で確認済み）:
> - board.js の `computeInputOverlay`（684–）と `computeRevealOverlay`（674–）は、`renderSvg(pos, overlay)`（`board-view.js`）が受ける overlay を作る計算。**両者とも DOM も Wasm も呼ばない**（純粋度は確認済み）。だが状態をモジュールスコープから暗黙に読む: `computeInputOverlay` は `selectedFrom`・`inputStep`・`legalTargets`、`computeRevealOverlay` は `kifu.plies`・`cursor`・`parseUsi`。
> - overlay の形（`renderSvg` の契約）: `{ board:[[f,r],…], sHand:Set|null, gHand:Set|null, legalDots:Set<"f,r">|null, selectedSquare:[f,r]|null }`。`computeInputOverlay` は必ず object を返し、`computeRevealOverlay` は `kifu.plies[cursor]` が無ければ `null` を返す。
> - `render()`（DOM を大量に書く巨大関数）はこの二つを呼び分ける（`phase==='reveal'` なら reveal、else 入力があれば input、無ければ `null`）。本書は **render() 本体には手を触れず**、overlay 計算だけを純粋化して `board-view.js` へ寄せる。
> - `parseUsi` は `usi.js`（`board-view.js` から import 可能）。
> - **設計の含意**: 第二段a/b と同じ規律で状態は動かさない。overlay 計算を「状態スナップショットを引数で受け、overlay を返す純粋関数」にして `board-view.js`（renderSvg の隣）へ置く。これは view 層の安全網の第一歩——将来 render() を「状態→描画」の純粋関数へ寄せるとき、overlay 部分が既にテストで固まっている状態を作る。
> 関連文書: `不完全将棋_実装指示書_着手組み立ての計算を純粋化_board分割第二段b`、`不完全将棋_実装指示書_純粋の収穫_board分割第一段a`（`board-view.js`・golden snapshot の初出）、`不完全将棋_バックログ_伏線と未決`。
> 性格: 第二段c は**「盤の overlay 計算（選択・開示の墨点/強調）を、状態を引数で受ける純粋関数として `board-view.js` へ寄せる」**。ロジックは無変更、変えるのは「モジュール状態を暗黙に読む」を「引数で受ける」に純粋化する一点。render() 本体・状態変数・状態機械は触らない。Rust に触れず Wasm 再ビルドなし。製品挙動は不変。行番号は v0.11.6 の board.js 基準の目安——**関数名で位置を特定**。

---

## 0. 目的と範囲

- **作るもの**:
  1. `web/board-view.js` に純粋関数二つを追加: `inputOverlay(sel)`（選択状態→overlay）と `revealOverlay(ply)`（開示する組手→overlay）。既存の `renderSvg` と同じモジュール（overlay は renderSvg の入力を作る計算なので、盤の view 層に同居させる）。
  2. `web/test/board-view.test.js`（既存）に overlay のテストを追加。Wasm 不要（純粋・状態注入）。
- **位置づけ**: board.js 分割の**第二段c**。view 層の純粋化の第一歩。overlay 計算を状態注入の純粋関数にして固定する。render() 本体の分解は後段（本丸＝状態集約の前後で扱う）。
- **作らないもの（＝理由つき）**:
  - **render() 本体の純粋化・分解**: DOM 書き込みが 20 箇所超（getElementById/textContent/disabled/hidden/classList/dataset）。巨大で、状態集約と絡む。**据え置き**（本丸の前後で扱う）。今は overlay 計算だけを抜く（最小・安全）。
  - **状態変数の移動**（`selectedFrom`/`inputStep`/`legalTargets`/`cursor`/`kifu`）: 据え置き（第二段a/b と同じ規律）。overlay 関数へは board.js が現在値を**引数で渡す**。
  - phaseText/ボタン制御ロジックの純粋化: render() 本体に属する。後段。

---

## 1. `web/board-view.js` に追加する純粋関数

`renderSvg` の下（同ファイル）に追加。状態はすべて引数で受け、モジュールスコープを読まない。`parseUsi` は import する。

```js
import { parseUsi } from './usi.js';   // 既存の geometry import に並べて追加
```

```js
// 選択状態から入力 overlay を作る（純粋）。renderSvg が受ける overlay の形を返す。
//   sel = { selectedFrom: {board:[f,r]} | {hand:kind} | null,
//           inputStep: 'sente'|'gote'|null,
//           legalTargets: Map<"f,r", …> | null }
export function inputOverlay(sel) {
  const overlay = { board: [], sHand: null, gHand: null, legalDots: null, selectedSquare: null };

  if (sel.selectedFrom?.board) {
    overlay.selectedSquare = sel.selectedFrom.board;
  } else if (sel.selectedFrom?.hand) {
    if (sel.inputStep === 'gote') {
      overlay.gHand = new Set([sel.selectedFrom.hand]);
    } else {
      overlay.sHand = new Set([sel.selectedFrom.hand]);
    }
  }

  if (sel.legalTargets) {
    overlay.legalDots = new Set(sel.legalTargets.keys());
  }

  return overlay;
}

// 開示する組手（ply = {sUsi, gUsi}）から reveal overlay を作る（純粋）。
// ply が無ければ null（呼び出し側で kifu.plies[cursor] を渡す）。
export function revealOverlay(ply) {
  if (!ply) return null;
  const s = parseUsi(ply.sUsi);
  const g = parseUsi(ply.gUsi);
  return {
    board:          [s.isDrop ? null : s.from, s.to, g.isDrop ? null : g.from, g.to].filter(Boolean),
    sHand:          s.isDrop ? new Set([s.kind]) : null,
    gHand:          g.isDrop ? new Set([g.kind]) : null,
    legalDots:      null,
    selectedSquare: null,
  };
}
```

- **ロジック無変更**の確認: `inputOverlay` は元 `computeInputOverlay` と同じ分岐。元は `overlay.gHand = overlay.gHand || new Set(); overlay.gHand.add(...)` だが、overlay は関数内で新規生成され gHand は初期 null・この分岐は一度きりなので `new Set([hand])` と等価（挙動不変）。`revealOverlay` は元 `computeRevealOverlay` と同一。

## 2. board.js 側の書き換え（状態を引数で渡すだけ）

`board-view.js` の import に追加（既存の `import { renderSvg } from './board-view.js';` を拡張）:

```js
import { renderSvg, inputOverlay, revealOverlay } from './board-view.js';
```

board.js から `computeInputOverlay`（684–）と `computeRevealOverlay`（674–）の**定義を削除**。render() 内の呼び出しを、現在の状態を引数で渡す形に置換:

```js
// render() 内、reveal 分岐:
overlay = revealOverlay(kifu.plies[cursor]);

// render() 内、else 分岐:
overlay = hasInput
  ? inputOverlay({ selectedFrom, inputStep, legalTargets })
  : null;
```

- render() の他の部分（phaseText・moveText・DOM 書き込み・ボタン制御）は**すべて無変更**。overlay を得る 2 行だけが差し替わる。
- `parseUsi` は board.js でも別途使われている（無変更）。`board-view.js` 側でも import するが二重定義にはならない（各モジュールが `usi.js` から import）。

## 3. テスト（`web/test/board-view.test.js` に追加・Wasm 不要）

既存の renderSvg テストの下に overlay のテストを追加。状態を直に作って注入（純粋・Wasm 足場不要）。

```js
import { renderSvg, inputOverlay, revealOverlay } from "../board-view.js";
// （既存 import 行を拡張）

describe("inputOverlay（選択状態→overlay）", () => {
  it("盤上選択は selectedSquare を立て、legalTargets は墨点になる", () => {
    const lt = new Map([["7,6", { options: [] }], ["7,5", { options: [] }]]);
    const ov = inputOverlay({ selectedFrom: { board: [7, 7] }, inputStep: "sente", legalTargets: lt });
    expect(ov.selectedSquare).toEqual([7, 7]);
    expect(ov.legalDots).toEqual(new Set(["7,6", "7,5"]));
    expect(ov.sHand).toBeNull();
  });

  it("先手の持ち駒選択は sHand に入る", () => {
    const ov = inputOverlay({ selectedFrom: { hand: "P" }, inputStep: "sente", legalTargets: null });
    expect(ov.sHand).toEqual(new Set(["P"]));
    expect(ov.gHand).toBeNull();
    expect(ov.selectedSquare).toBeNull();
  });

  it("後手の持ち駒選択は gHand に入る", () => {
    const ov = inputOverlay({ selectedFrom: { hand: "P" }, inputStep: "gote", legalTargets: null });
    expect(ov.gHand).toEqual(new Set(["P"]));
    expect(ov.sHand).toBeNull();
  });

  it("選択なしなら空 overlay（board 空・全 null）", () => {
    const ov = inputOverlay({ selectedFrom: null, inputStep: null, legalTargets: null });
    expect(ov.selectedSquare).toBeNull();
    expect(ov.legalDots).toBeNull();
    expect(ov.board).toEqual([]);
  });
});

describe("revealOverlay（開示する組手→overlay）", () => {
  it("ply が無ければ null", () => {
    expect(revealOverlay(undefined)).toBeNull();
  });

  it("盤上の手は from/to、打ちは to のみ＋持ち駒 Set", () => {
    const ov = revealOverlay({ sUsi: "7g7f", gUsi: "P*5e" });
    // sente 7g7f: from[7,7] to[7,6] 両方 board に入る（drop でない）
    // gote  P*5e: to[5,5] のみ（from は null で除外）、gHand に P
    expect(ov.board).toContainEqual([7, 7]);
    expect(ov.board).toContainEqual([7, 6]);
    expect(ov.board).toContainEqual([5, 5]);
    expect(ov.gHand).toEqual(new Set(["P"]));
    expect(ov.sHand).toBeNull();
  });
});
```

- overlay の renderSvg への渡り（overlay 経由で墨点・強調が描画されること）は既存の golden snapshot が引き続き守る。ここでは overlay 計算自体の正しさを直接固定する。

## 4. 受け入れ

- `cd web && npm test` が緑（既存 36 件＋新規 overlay 6 件、warn なし）。
- ブラウザで従来通り: 駒/持ち駒選択時の強調（selectedSquare）と合法手の墨点（legalDots）、同時開示時の着手ハイライト（board の from/to・打ちの持ち駒 Set）。入力なし局面で overlay が出ないこと。
- **特に確認**: 後手の持ち駒選択で gHand 側が光ること（sHand/gHand の左右）、打ち（drop）の開示で from が出ず to のみ光ること。

## 5. 版の刻み

- **製品挙動は不変・Rust 非関与・Wasm 再ビルドなし**。第二段 a/b と同じ扱い: 配布版据え置き **v0.11.2**、web の `?v=`（`web/package.json`・`web/index.html`）を **0.11.7** へ前進（board.js が `board-view.js` から新規 import を増やすためキャッシュ更新）。**RULE 0.6・PROTOCOL 4・アーカイブ書式 1 不変**。

## 6. 申し送り（本丸へ）

- view 層の純粋化の**第一歩**が済んだ（overlay 計算が状態注入の純粋関数として固定）。次は render() 本体——phaseText を決める大きな分岐（`bothReady`/`pendingSente`/`inputStep`/`watchMode`/`onlineMode`…）を「状態スナップショット→表示文字列」の純粋関数へ、ボタン disabled ロジックを「状態→ボタン状態」の純粋関数へ寄せると、render() が「純粋に決めた表示値を DOM へ流し込むだけ」に痩せる。
- **本丸＝状態集約**は、view が「状態スナップショット→描画」の純粋関数群になった後にやると、集約前後で「同じスナップショット→同じ描画」をテストで守れる（各段が次段の安全網）。`setRecord`/`currentRecord`（第二段a）と本段の overlay 関数が、そのスナップショットの芽。
- golden snapshot への局面追加（第一段a の申し送り）は、renderSvg を触る次の view 段で。

---

## 7. 不変の原則（本実装が守るもの）

1. **状態は動かさず、計算を純粋化する**: overlay 計算をモジュール状態依存から引数注入へ。`selectedFrom`/`inputStep`/`legalTargets`/`cursor`/`kifu` は board.js 据え置き。
2. **ロジック無変更**: `inputOverlay`/`revealOverlay` は元 `computeInputOverlay`/`computeRevealOverlay` と同じ結果（gHand/sHand の生成の書き方だけ等価に整理）。
3. **render() 本体は触らない**: overlay を得る 2 行だけ差し替え。phaseText・DOM 書き込み・ボタン制御は無変更。本丸（状態集約）と render() 分解は後段。
4. **盤の view に同居**: overlay は renderSvg の入力を作る計算なので `board-view.js` に置く（「ピクセルと、ピクセルの入力を決める計算」を盤に集める）。
5. **Rust に触れず Wasm を再ビルドしない**: 純粋 JS ＋テストのみ。配布版据え置き、web `?v=` のみ前進。

---

*第二段c——overlay 計算を純粋化して盤へ寄せる。view 分離の本体（render の DOM 書き込み 20 箇所超）は巨大で状態集約と絡むので触らず、最も純粋な部分＝選択・開示の overlay 計算だけを先に抜く。元は `selectedFrom`/`cursor` 等をモジュールスコープから暗黙に読んでいたのを、状態スナップショットを引数で受ける純粋関数（`inputOverlay`/`revealOverlay`）にして `board-view.js`（renderSvg の隣）へ置く。ロジックは無変更、render 本体は overlay を得る 2 行だけ差し替え。Wasm 不要でテストが固まり、これが本丸＝状態集約の安全網の第一歩になる——view が「状態→描画」の純粋関数になれば、集約前後で「同じ状態→同じ描画」を守れる。急がば回れ。*
