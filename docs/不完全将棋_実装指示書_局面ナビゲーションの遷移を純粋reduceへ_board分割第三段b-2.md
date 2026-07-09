# 不完全将棋 実装指示書 — 第三段b-2：局面ナビゲーションの遷移を純粋 reduce へ（書き込み集約の第一号）

> 対象実行者: Claude Code（Sonnet 5 または Haiku 4.5）
> 前提: 配布 v0.11.2 / web `?v=`0.11.9（board.js 分割 第三段b-1 まで着地。状態は単一の `state`〔`plies` まで吸収済み〕、更新は `update(patch)`〔浅いマージ＋`render()`〕を通る。web テスト 42 件・golden snapshot・純粋モジュール 7 本が据わっている）。
> 関連する現物（すべて実地で確認済み）:
> - 局面ナビゲーションは `goPrev`/`goNext`（board.js）。両者に**純粋に写せる遷移**と**副作用が絡む分岐**が混在:
>   - 純粋（reduce へ）: `phase==='reveal'→'position'`（prev、cursor 据え置き）／`phase==='position' && cursor>0 → cursor-1, 'reveal'`（prev）／`phase==='position' && cursor<plies.length → 'reveal'`（next）／`phase==='reveal' → cursor+1, 'position'`（next）。ガード `onlineMode && !onlineGameOver` はナビ不可（状態不変）。
>   - 副作用（reduce の外・board.js に残す）: `promotionPending` クリア（`hidePromotionUI`＝DOM）／入力キャンセル（`resetInput`）／`pendingSente && pendingGote` の解決（`branchAndAppend`＝Wasm＋棋譜遷移）。
> - **実地検証済み**（node で `navReduce` を試作）: 全 8 分岐が期待通り。局面往復は `0:reveal → 1:position → 1:reveal → 2:position → …`（各組手が reveal→position の二拍で刻まれる）。patch を返し、不可/不変なら `null` を返す形が `update` と噛み合う。
> - **相似形の北極星（tui）と本段の位置づけ**: `tui/src/app.rs` は `undo`/`on_escape` を `&mut self` メソッドで書いている。**tui は中間形態**であり目標そのものではない——web はより純粋な形（状態を受けて patch を**返す**純粋関数）へ進む。web の純粋 `reduce` は、将来 tui 側をも純粋方向へ引き上げる先例になる。深層構造（遷移が一箇所に集約され描画と分離）は tui と同じで、移植時 `reduce` は `&mut self` へ素直に翻訳できる。
> 関連文書: `不完全将棋_実装指示書_kifu吸収と状態更新経路_board分割第三段b-1`、`不完全将棋_実装指示書_着手組み立ての計算を純粋化_board分割第二段b`（純粋関数の流儀）、`不完全将棋_バックログ_伏線と未決`。
> 性格: 第三段b-2 は**「局面ナビゲーションの純粋な遷移を `reduce` 関数へ抜き、テストで充実させる。書き込み集約の第一号」**。形は**純粋 reducer（`(state, action) → patch | null`）**——これまでの純粋モジュール（game-record・move-input・overlay）と同じ「値を受け値を返す」流儀で統一する。副作用（DOM・Wasm・棋譜遷移）は board.js のラッパに残す。種類1 の残り（`_resetOnlineState`・ホットシート確定）は続く小段へ。Rust に触れず Wasm 再ビルドなし。製品挙動は不変。行番号は v0.11.9 基準。

---

## 0. 目的と範囲

- **作るもの**:
  1. `web/nav.js` — 局面ナビゲーションの純粋遷移 `navReduce(view, action)`。状態の必要部分だけを受け、次状態の patch（変化分オブジェクト）か `null`（不可・不変）を返す。Wasm も DOM も状態も触れない。
  2. `web/test/nav.test.js` — 全分岐と局面往復を検証（Wasm 不要）。
  3. board.js の `goPrev`/`goNext` を、`navReduce` を呼んで patch を `update` する形に整理（副作用分岐は残す）。
- **位置づけ**: board.js 分割の**第三段b-2**。書き込み集約の第一号——意味のある状態遷移を純粋関数として固める。局面ナビゲーションは「局面を行き来する」状態機械の核で、テスト価値が最も高い（案A以来「最も価値ある芯から最小で」）。
- **作らないもの（＝理由つき）**:
  - **種類1 の残りの純粋化**（`_resetOnlineState` の 11 変数リセット、`confirmMove` のホットシート確定）: 続く小段（b-3 等）。本段はナビゲーションに絞る。
  - **副作用分岐の純粋化**: `promotionPending` クリア（DOM）・`resetInput`・`branchAndAppend`（Wasm）は reduce の外。board.js のラッパに残す。
  - **種類2（I/O 絡み）の分解**（`handleTurnComplete`・`enterWatchMode`・`endOnlineGame`）: tui の online.rs 相当の分離は後段。
  - **action の統一ディスパッチャ**（全遷移を 1 つの巨大 reduce へ）: 過ぎたるは及ばざる。まずナビの小さな純粋関数から。汎化は必要が呼んでから。

---

## 1. `web/nav.js`（純粋・状態も DOM も Wasm も触れない）

`navReduce` は状態の**必要部分だけ**を受ける（全 `state` を渡さない——依存を最小化し、テストを軽くする）。返すのは patch か `null`。

```js
// 局面ナビゲーションの純粋な状態遷移。view = 状態の必要部分、action = 'prev' | 'next'。
// 次状態の patch（変化分）を返す。ナビ不可・状態不変なら null（呼び出し側は再描画のみ）。
// board.js 分割 第三段b-2。DOM・Wasm・可変状態に非依存。
//   view = { phase:'position'|'reveal', cursor:number, pliesLen:number,
//            onlineMode:boolean, onlineGameOver:boolean }
export function navReduce(view, action) {
  // オンライン対局中（終局前）はナビ不可。
  if (view.onlineMode && !view.onlineGameOver) return null;

  if (action === 'prev') {
    if (view.phase === 'reveal') return { phase: 'position' };
    if (view.phase === 'position' && view.cursor > 0) return { cursor: view.cursor - 1, phase: 'reveal' };
    return null;
  }
  if (action === 'next') {
    if (view.phase === 'position' && view.cursor < view.pliesLen) return { phase: 'reveal' };
    if (view.phase === 'reveal') return { cursor: view.cursor + 1, phase: 'position' };
    return null;
  }
  return null;
}
```

- **純粋**（引数のみに依存、Wasm も DOM も状態も触らない）。`cursor 据え置き`（reveal→position の prev）は patch に cursor を含めない＝`{ phase:'position' }` のみ、で表現（`update` の浅いマージで cursor は不変のまま）。

## 2. board.js 側の `goPrev`/`goNext` の整理

`navReduce` を import。ナビの純粋遷移部分だけを `navReduce` へ委譲し、副作用分岐（promotion クリア・入力キャンセル・pending 解決）は残す。

```js
import { navReduce } from './nav.js';
```

```js
function goPrev() {
  // 副作用分岐（純粋化しない）: 入力途中のキャンセルを優先
  if (state.onlineMode && !state.onlineGameOver) return;   // ナビ不可（navReduce と同判定だが早期 return）
  if (state.promotionPending) {
    hidePromotionUI();
    update({ promotionPending: null, selectedFrom: null, legalTargets: null });
    return;
  }
  if (state.inputStep !== null || state.selectedFrom !== null) {
    resetInput();
    render();
    return;
  }
  // 純粋なナビ遷移
  const patch = navReduce(navView(), 'prev');
  if (patch) update(patch); else render();
}

function goNext() {
  if (state.promotionPending) return;
  if (state.onlineMode && !state.onlineGameOver) return;
  // 副作用分岐: 両者着手済みなら解決（棋譜へ追記）
  if (state.pendingSente && state.pendingGote) {
    branchAndAppend(state.pendingSente.usi, state.pendingGote.usi, state.pendingSente.text, state.pendingGote.text);
    render(); return;
  }
  // 純粋なナビ遷移
  const patch = navReduce(navView(), 'next');
  if (patch) update(patch); else render();
}
```

`navView()` は `state` から navReduce の入力を切り出す小ヘルパ（board.js に置く）:

```js
function navView() {
  return {
    phase: state.phase, cursor: state.cursor, pliesLen: state.plies.length,
    onlineMode: state.onlineMode, onlineGameOver: state.onlineGameOver,
  };
}
```

- **挙動保存の確認**: 元の `goPrev`/`goNext` の分岐順と結果を厳密に保つ。
  - `goPrev`: online ガード → promotion クリア → 入力キャンセル → ナビ（reveal→position / position→前 reveal / それ以外は再描画のみ）。navReduce の online 判定は早期 return と重複するが、到達時は必ず navigable なので結果同一。
  - `goNext`: promotion ガード → online ガード → pending 解決 → ナビ（position→reveal / reveal→次 position / それ以外は再描画のみ）。
  - `null` のとき `render()` を呼ぶ（元の `else { render(); }` と同じ＝状態不変でも再描画）。

## 3. テスト `web/test/nav.test.js`（充実させる・Wasm 不要）

局面ナビの状態機械を厚く固める。往復の可逆性・境界（初期/最終局面）・オンラインガードを網羅。

```js
import { describe, it, expect } from "vitest";
import { navReduce } from "../nav.js";

const V = (o = {}) => ({
  phase: 'position', cursor: 0, pliesLen: 5,
  onlineMode: false, onlineGameOver: false, ...o,
});

describe("navReduce（局面ナビゲーションの純粋遷移）", () => {
  it("prev: reveal → position（cursor 据え置き）", () => {
    expect(navReduce(V({ phase: 'reveal', cursor: 2 }), 'prev')).toEqual({ phase: 'position' });
  });
  it("prev: position(cursor>0) → 前の reveal", () => {
    expect(navReduce(V({ phase: 'position', cursor: 2 }), 'prev')).toEqual({ cursor: 1, phase: 'reveal' });
  });
  it("prev: 初期局面（cursor 0・position）は不可 → null", () => {
    expect(navReduce(V({ phase: 'position', cursor: 0 }), 'prev')).toBeNull();
  });
  it("next: position → reveal", () => {
    expect(navReduce(V({ phase: 'position', cursor: 2 }), 'next')).toEqual({ phase: 'reveal' });
  });
  it("next: reveal → 次の position", () => {
    expect(navReduce(V({ phase: 'reveal', cursor: 2 }), 'next')).toEqual({ cursor: 3, phase: 'position' });
  });
  it("next: 最終局面（cursor===pliesLen・position）は不可 → null", () => {
    expect(navReduce(V({ phase: 'position', cursor: 5, pliesLen: 5 }), 'next')).toBeNull();
  });

  it("オンライン対局中（終局前）はナビ不可 → null（prev/next とも）", () => {
    const v = V({ phase: 'reveal', cursor: 2, onlineMode: true, onlineGameOver: false });
    expect(navReduce(v, 'prev')).toBeNull();
    expect(navReduce(v, 'next')).toBeNull();
  });
  it("オンライン終局後はナビ可", () => {
    const v = V({ phase: 'reveal', cursor: 2, onlineMode: true, onlineGameOver: true });
    expect(navReduce(v, 'prev')).toEqual({ phase: 'position' });
  });

  it("往復の可逆性: next で進んで prev で戻ると元へ", () => {
    let s = { phase: 'position', cursor: 1, pliesLen: 5 };
    const apply = (a) => { const p = navReduce({ ...s, onlineMode: false, onlineGameOver: false }, a); if (p) Object.assign(s, p); };
    apply('next'); // position→reveal
    apply('next'); // reveal→cursor2 position
    expect(s).toMatchObject({ cursor: 2, phase: 'position' });
    apply('prev'); // →cursor1 reveal
    apply('prev'); // reveal→position
    expect(s).toMatchObject({ cursor: 1, phase: 'position' });
  });

  it("各組手は reveal→position の二拍で刻まれる（同時着手の歩み）", () => {
    let s = { phase: 'position', cursor: 0, pliesLen: 3 };
    const trail = [];
    for (let i = 0; i < 6; i++) {
      const p = navReduce({ ...s, onlineMode: false, onlineGameOver: false }, 'next');
      if (!p) break; Object.assign(s, p); trail.push(`${s.cursor}:${s.phase}`);
    }
    expect(trail).toEqual(['0:reveal', '1:position', '1:reveal', '2:position', '2:reveal', '3:position']);
  });

  it("未知の action は null", () => {
    expect(navReduce(V(), 'jump')).toBeNull();
  });
});
```

## 4. 受け入れ

- `cd web && npm test` が緑（既存 42 件＋新規 `nav` 11 件、warn なし。snapshot 差分ゼロ）。
- ブラウザで従来通り: 棋譜ナビ（← →・ボタン）で局面が reveal↔position を二拍で行き来、初期局面で prev 止まり・最終局面で next 止まり、入力途中の ← で入力キャンセル（1 回目）→ ナビ開始（2 回目）、成り選択中の ← で選択解除、両者着手済みの → で解決（棋譜追記）、オンライン対局中はナビ無効・終局後は有効。
- **特に確認**: 副作用分岐（promotion クリア・resetInput・pending 解決）が純粋遷移に吸収されず従来通り動くこと。`navReduce` が `null` を返す局面で再描画のみ行われること。

## 5. 版の刻み

- **製品挙動は不変・Rust 非関与・Wasm 再ビルドなし**。第三段の他段と同じ扱い: 配布版据え置き **v0.11.2**、web の `?v=`（`web/package.json`・`web/index.html`）を **0.11.10** へ前進（board.js が `nav.js` を新規 import するためキャッシュ更新）。**RULE 0.6・PROTOCOL 4・アーカイブ書式 1 不変**。

## 6. 申し送り（種類1 の残り・種類2・view へ）

- 局面ナビの純粋遷移が固定された（書き込み集約の第一号）。次の種類1: `_resetOnlineState`（11 変数のリセット＝`resetOnlineReduce()` が返す定数 patch）、`confirmMove` のホットシート確定（pending セット・inputStep 進行の純粋遷移＋DOM/commit 副作用の分離）。同じ「純粋 reduce＋board.js ラッパ」で。
- 種類2（`handleTurnComplete`・`enterWatchMode`・`endOnlineGame`）は tui の online.rs に倣い「純粋な状態遷移」と「I/O」に割る。純粋部分は reduce へ、I/O は殻に残す。
- **view の純粋化**（render() 本体の phaseText/ボタン分岐を `state` スナップショット→表示値の純粋関数へ）は、遷移群が固まった後に。集約前後で「同じ `state` → 同じ描画」を守れる。`navView()` のような「state から必要部分を切り出す」パターンが、view スナップショットの芽。
- golden snapshot への局面追加（第一段a の申し送り）は view 段で。

---

## 7. 不変の原則（本実装が守るもの）

1. **純粋 reducer で統一**: `navReduce(view, action) → patch | null`。値を受け値を返す（game-record・move-input・overlay と同じ流儀）。tui のメソッド（中間形態）より純粋な形へ進む。
2. **依存は最小**: 全 `state` でなく必要部分（`navView()`）だけを渡す。テストが軽く、依存が明示される。
3. **副作用は reduce の外**: DOM（promotion）・入力キャンセル（resetInput）・棋譜遷移（branchAndAppend）は board.js のラッパに残す。純粋遷移だけ抜く。
4. **挙動保存**: `goPrev`/`goNext` の分岐順・結果・`null` 時の再描画を厳密に保つ。
5. **最小から**: ナビゲーションに絞る。種類1 の残り・種類2・統一ディスパッチャは後段。過ぎたるは及ばざる。Rust に触れず Wasm 再ビルドなし。配布版据え置き、web `?v=` のみ前進。

---

*第三段b-2——局面ナビゲーションの遷移を純粋 reduce へ。書き込み集約の第一号。tui は中間形態ゆえ、その `&mut self` メソッドより純粋な形（状態を受け patch を返す `navReduce`）へ進む——web の純粋 reduce が将来 tui をも純粋方向へ引き上げる先例になる。局面ナビ（reveal↔position・cursor 増減・オンラインガード）の純粋遷移だけを `nav.js` へ抜き、副作用（promotion の DOM・入力キャンセル・pending 解決の Wasm）は board.js のラッパに残す。patch を返し不可なら null——`update` 経路と噛み合う。全 8 分岐と往復の可逆性、各組手が reveal→position の二拍で刻まれる同時着手の歩みを、テストで充実させる（node で実証済み）。深層構造は tui と同じ、表層はより純粋——移植時 reduce は &mut self へ素直に翻訳できる。最も価値ある状態機械の核から、最小で詰める。*
