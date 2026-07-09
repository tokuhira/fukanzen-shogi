# 不完全将棋 実装指示書 — 第二段a：棋譜コアの遷移を純粋化する（`game-record.js`）

> 対象実行者: Claude Code（Sonnet 5 または Haiku 4.5）
> 前提: 配布 v0.11.2 / web `?v=`0.11.4（board.js 分割 第一段b まで着地。web テスト 23 件・Wasm-in-node 足場〔`test/wasm-loader.js`〕・注入パターン〔`notation-view.js` の純粋モジュール＋board.js の薄いラッパ〕が据わっている）。
> 関連する現物（すべて実地で確認済み）:
> - 棋譜コアの大域可変状態は board.js の `sfens`（74）・`events`（75）・`cursor`（73）・`phase`（76）＋ `const kifu = { plies: [] }`（72）。参照は広い（`cursor` 53・`phase` 34・`sfens`/`kifu.plies` 各 24 箇所）。**本書はこの状態変数を移動しない**——参照の広さゆえ、移動は blast radius が大きく「小さく安全」に反する。移すのは**遷移の純粋計算だけ**。
> - 純粋化する遷移: `loadPlies`（127–148）・`branchAndAppend`（161–178）・`watchAppendTurn`（181–192）。三者の芯は「（前の sfens/events/plies 列, 着手）→ 新しい列」という純粋計算で、間に `resolve_ply`（engine-wasm）と `usiToText`（notation の糊）を挟むだけ。DOM には触れない。混ざる不純は `resetInput()`（入力島への副作用）・`cursor`/`phase` 代入・`gameOverCache`/`resultOverride`/`loadedMeta` リセットのみ——これらは board.js のラッパに残す。
> - **branch と watch の差**: `branchAndAppend` は切り詰め位置に `cursor` を使う（`sfens.slice(0, cursor+1)`＝レビュー中の分岐）。`watchAppendTurn` は `kifu.plies.length` を使う（末尾追記・cursor 非依存）。純粋の芯は**切り詰め位置を引数で受けて**両方を吸収する。
> - `resolve_ply(sfen, sente_usi, gote_usi): string`（JSON、`{ok, sfen, event}` or `{ok:false, error}`）。`usiToText` は第一段b の注入形。
> - **実地検証済み**（node で実 Wasm）: 不変な `applyTurn` で初期→1手後に sfens/events/plies が 2/1/1 に育ち、棋譜 `☗７六歩`/`☖３四歩` が出る。切り詰め＋適用で分岐が効き、**元の状態オブジェクトは不変**（新配列を返す）。
> 関連文書: `不完全将棋_実装指示書_Wasm足場と棋譜の糊_board分割第一段b`（注入パターンの雛形）、`不完全将棋_バックログ_伏線と未決`。
> 性格: 第二段a は**「棋譜コアの遷移（値の計算）を純粋モジュールへ抜き、実 Wasm でテストして固める」**。状態変数は board.js に据え置き、遷移関数は「純粋計算を呼び、返り値を状態へ代入し、副作用を足す薄いラッパ」に変わる（第一段b の `usiToText` と同じ構図）。**返り値は不変**（新配列を返す・破壊的 push はしない）。Rust に触れず Wasm 再ビルドなし。製品挙動は不変。行番号は v0.11.4 の board.js 基準の目安——**関数名で位置を特定**。

---

## 0. 目的と範囲

- **作るもの**:
  1. `web/game-record.js` — 棋譜コアの純粋な遷移計算。Wasm（`resolvePly`）と `usiToText` は**引数注入**。状態を持たず、`{ sfens, events, plies }`（＝以下 `record`）を受けて**新しい `record` を返す**（不変）。
  2. `web/test/game-record.test.js` — Wasm-in-node 足場で実 `resolve_ply` を注入し、遷移を検証。
- **位置づけ**: board.js 分割の**第二段a**。状態島の物理移動ではなく、遷移の純粋化とテストによる固定。ここで棋譜コアの遷移が錠されると、第二段b 以降で状態そのものを動かすとき（view 分離が要求したら）安全に動かせる。
- **作らないもの（＝理由つき）**:
  - **状態変数（`sfens`/`events`/`cursor`/`phase`/`kifu`）の移動**: 参照が広い（`cursor` 53 箇所等）。移すと第二段a が巨大化する。**据え置き**。動かすのは必要が呼ぶまで（過ぎたるは及ばざる）。
  - **`resetToNew`（149–159）の純粋化**: これは「初期状態を作る」だけで着手適用が無く、Wasm も呼ばない。`record` の初期値生成（`emptyRecord(initialSfen)`）だけ `game-record.js` に置き、リセット時の副作用（`loadedMeta=null` 等）は board.js に残す。
  - **入力島・オンライン島・観戦島の状態**、view（`render`）の分離: 後段。
  - `gameOverCache`/`resultOverride`/`loadedMeta` の管理: board.js のラッパに残す（棋譜コアの純粋計算の外）。

---

## 1. `web/game-record.js`（純粋・Wasm 注入・不変）

`record = { sfens, events, plies }`。`plies[i] = { sUsi, gUsi, sText, gText }`。すべての関数は新しい `record` を返し、引数の `record` を変更しない。

```js
// 棋譜コアの純粋な遷移。record = { sfens, events, plies } を受けて新しい record を返す
// （不変・破壊的変更なし）。Wasm（resolvePly）と usiToText は呼び出し側から注入する
// （本モジュールは Wasm に直接依存しない）。board.js 分割 第二段a。
//
// resolvePly(sfen, sUsi, gUsi) -> { ok, sfen, event } | { ok:false, error }
// usiToText(usi, sfen, side)   -> 日本語棋譜テキスト（side: 'sente' | 'gote'）

export function emptyRecord(initialSfen) {
  return { sfens: [initialSfen], events: [], plies: [] };
}

// record の末尾に一組手を適用した新しい record を返す。sText/gText が渡されなければ導出。
export function appendTurn(record, sUsi, gUsi, resolvePly, usiToText, sText, gText) {
  const preSfen = record.sfens.at(-1);
  const r = resolvePly(preSfen, sUsi, gUsi);
  if (!r.ok) throw new Error(r.error);
  return {
    sfens:  [...record.sfens, r.sfen],
    events: [...record.events, r.event],
    plies:  [...record.plies, {
      sUsi, gUsi,
      sText: sText ?? usiToText(sUsi, preSfen, 'sente'),
      gText: gText ?? usiToText(gUsi, preSfen, 'gote'),
    }],
  };
}

// record を組手数 n までに切り詰めた新しい record を返す（sfens は n+1 本＝局面列）。
export function truncateTo(record, n) {
  return {
    sfens:  record.sfens.slice(0, n + 1),
    events: record.events.slice(0, n),
    plies:  record.plies.slice(0, n),
  };
}

// plies 列から record を組み立て直す（loadPlies の芯）。各 ply は {sUsi,gUsi,sText?,gText?}。
export function buildFromPlies(initialSfen, plies, resolvePly, usiToText) {
  let record = emptyRecord(initialSfen);
  for (const ply of plies) {
    record = appendTurn(record, ply.sUsi, ply.gUsi, resolvePly, usiToText, ply.sText, ply.gText);
  }
  return record;
}
```

- `appendTurn` の `resolvePly` は **JSON パース済みオブジェクト**を返す注入関数（board.js 側で `(sfen,s,g)=>JSON.parse(resolve_ply(sfen,s,g))` を渡す）。モジュール内で JSON.parse しない（Wasm 非依存を保つ）。
- `branchAndAppend` の「切り詰めてから適用」は `appendTurn(truncateTo(record, cursor), ...)` で表現。`watchAppendTurn` の「末尾へ追記」は `appendTurn(record, ...)`（切り詰めなし）。両者が同じ純粋部品の合成になる。

## 2. board.js 側の書き換え（状態は据え置き・ラッパで包む）

状態変数（`sfens`/`events`/`cursor`/`phase`/`kifu`）は**現在の宣言のまま**。三つの遷移関数を、純粋計算を呼んで結果を状態へ代入する薄いラッパに変える。import 追加:

```js
import { emptyRecord, appendTurn, truncateTo, buildFromPlies } from './game-record.js';
```

board.js ローカルに、実 Wasm を綴じた注入口を一つ（第一段b の `usiToText` ラッパの隣に置くと分かりやすい）:

```js
// 純粋な game-record へ実 Wasm を渡す注入口（resolve_ply は JSON パースして渡す）。
const resolvePly = (sfen, sUsi, gUsi) => JSON.parse(resolve_ply(sfen, sUsi, gUsi));
```

`record` と board.js の三配列（`sfens`/`events`/`kifu.plies`）の間は、**代入で橋渡し**する小ヘルパを置くと安全（三本を一度に差し替え、取り違えを防ぐ）:

```js
function setRecord(record) {
  sfens      = record.sfens;
  events     = record.events;
  kifu.plies = record.plies;
}
function currentRecord() {
  return { sfens, events, plies: kifu.plies };
}
```

各遷移のラッパ（副作用＝`resetInput`/`cursor`/`phase`/キャッシュは board.js に残す）:

```js
function loadPlies(plies, initialSfen = INITIAL_SFEN) {
  setRecord(buildFromPlies(initialSfen, plies, resolvePly, usiToText));
  cursor = 0;
  phase  = 'position';
  resetInput();
  gameOverCache  = { cursor: -1, msg: null };
  resultOverride = null;
}

function branchAndAppend(sUsi, gUsi, sText, gText) {
  setRecord(appendTurn(truncateTo(currentRecord(), cursor), sUsi, gUsi, resolvePly, usiToText, sText, gText));
  gameOverCache = { cursor: -1, msg: null };
  phase = 'reveal';  // cursor stays — reveal shows the move just played
  resetInput();
}

function watchAppendTurn(sUsi, gUsi) {
  try {
    setRecord(appendTurn(currentRecord(), sUsi, gUsi, resolvePly, usiToText));
  } catch (e) {
    console.error('watch: resolve_ply failed:', e.message);
    return;
  }
  gameOverCache = { cursor: -1, msg: null };
}
```

- **`watchAppendTurn` の挙動保存に注意**: 元は `resolve_ply` 失敗時に `console.error(...); return;`（例外を投げない）。純粋 `appendTurn` は `throw` するので、ラッパで try/catch し、**元と同じく握り潰して return**する（上記の通り）。`branchAndAppend`/`loadPlies` は元が `throw new Error` なので try/catch せずそのまま伝播（挙動保存）。
- `resetToNew`（149–159）は着手適用が無いので `setRecord(emptyRecord(INITIAL_SFEN))` に置換し、残りの副作用（`cursor`/`phase`/`resetInput`/キャッシュ/`loadedMeta=null`）はそのまま:
```js
function resetToNew() {
  setRecord(emptyRecord(INITIAL_SFEN));
  cursor = 0;
  phase  = 'position';
  resetInput();
  gameOverCache  = { cursor: -1, msg: null };
  resultOverride = null;
  loadedMeta     = null;
}
```
- `evaluateTerminalAt`（205–）等、`sfens`/`kifu.plies` を**読むだけ**の箇所は無変更（状態は同じ場所にある）。53 箇所の `cursor` 参照も無変更。

## 3. テスト `web/test/game-record.test.js`

Wasm-in-node 足場で実 `resolve_ply` と `usiToText`（engine＋notation）を注入し、純粋遷移を検証。

```js
import { describe, it, expect, beforeAll } from "vitest";
import { emptyRecord, appendTurn, truncateTo, buildFromPlies } from "../game-record.js";
import { loadEngine, loadNotation } from "./wasm-loader.js";

const INITIAL = "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1";
let resolvePly, usiToText;
beforeAll(async () => {
  const engine = await loadEngine();
  const notation = await loadNotation();
  resolvePly = (sfen, s, g) => JSON.parse(engine.resolve_ply(sfen, s, g));
  usiToText  = (usi, sfen, side) =>
    (side === "sente" ? "☗" : "☖") +
    notation.ja_notation(usi, side, engine.legal_actions(sfen, side), sfen);
});

describe("game-record（純粋遷移）", () => {
  it("emptyRecord は初期局面 1 本・空の events/plies", () => {
    const r = emptyRecord(INITIAL);
    expect(r.sfens).toEqual([INITIAL]);
    expect(r.events).toEqual([]);
    expect(r.plies).toEqual([]);
  });

  it("appendTurn で sfens/events/plies が 1 組手ぶん育ち、棋譜が導出される", () => {
    const r = appendTurn(emptyRecord(INITIAL), "7g7f", "3c3d", resolvePly, usiToText);
    expect(r.sfens.length).toBe(2);
    expect(r.events.length).toBe(1);
    expect(r.plies.length).toBe(1);
    expect(r.plies[0].sText).toBe("☗７六歩");
    expect(r.plies[0].gText).toBe("☖３四歩");
  });

  it("appendTurn は引数の record を変更しない（不変）", () => {
    const base = emptyRecord(INITIAL);
    appendTurn(base, "7g7f", "3c3d", resolvePly, usiToText);
    expect(base.sfens.length).toBe(1);  // 元は不変
    expect(base.plies.length).toBe(0);
  });

  it("渡した sText/gText はそのまま使われる（再計算しない）", () => {
    const r = appendTurn(emptyRecord(INITIAL), "7g7f", "3c3d", resolvePly, usiToText, "S", "G");
    expect(r.plies[0].sText).toBe("S");
    expect(r.plies[0].gText).toBe("G");
  });

  it("truncateTo で n 組手＋局面 n+1 本に切り詰まる", () => {
    let r = buildFromPlies(INITIAL, [
      { sUsi: "7g7f", gUsi: "3c3d" },
      { sUsi: "2g2f", gUsi: "8c8d" },
    ], resolvePly, usiToText);
    expect(r.plies.length).toBe(2);
    const t = truncateTo(r, 1);
    expect(t.plies.length).toBe(1);
    expect(t.sfens.length).toBe(2);
    expect(t.events.length).toBe(1);
    expect(r.plies.length).toBe(2);  // 元は不変
  });

  it("buildFromPlies は plies 列から record を組み直す", () => {
    const r = buildFromPlies(INITIAL, [{ sUsi: "7g7f", gUsi: "3c3d" }], resolvePly, usiToText);
    expect(r.sfens.length).toBe(2);
    expect(r.plies[0].sText).toBe("☗７六歩");
  });

  it("不正手は resolvePly が ok:false を返し appendTurn が throw", () => {
    expect(() => appendTurn(emptyRecord(INITIAL), "9i9a", "1a1i", resolvePly, usiToText)).toThrow();
  });
});
```

## 4. 受け入れ

- `cd web && npm test` が緑（既存 23 件＋新規 `game-record` 7 件、warn なし）。
- ブラウザで従来通り: 新規対局・棋譜読み込み（`loadPlies`）・着手確定と開示（`branchAndAppend` → phase='reveal'）・観戦のライブ追記（`watchAppendTurn`、レビュー中も末尾に積まれる）・分岐（過去局面から指し直すと以降が切り詰められる）・アーカイブ保存/読み込み・終局判定。
- **特に確認**: `branchAndAppend` 後に `phase='reveal'` かつ `cursor` 据え置き（開示表示）、`watchAppendTurn` の失敗時に例外が漏れず握り潰される（元挙動）、`loadPlies`/`resetToNew` 後に `cursor=0`。

## 5. 版の刻み

- **製品挙動は不変・Rust 非関与・Wasm 再ビルドなし**。第一段と同じ扱い: 配布版据え置き **v0.11.2**、web の `?v=`（`web/package.json`・`web/index.html`）を **0.11.5** へ独立に前進（board.js が `game-record.js` を新規 import するためキャッシュ更新）。**RULE 0.6・PROTOCOL 4・アーカイブ書式 1 不変**。

## 6. 申し送り（第二段b 以降へ）

- 棋譜コアの**遷移**が純粋化・テスト固定された。次に状態そのもの（`cursor`/`phase`/`record`）を動かしたくなったら（view 分離が「状態を一箇所に集めて購読したい」と要求したら）、遷移が固まっているので安全に移せる。`setRecord`/`currentRecord` が状態と純粋層の境界の芽になっている。
- 入力島（`inputStep`/`pendingSente`/…/`selectedFrom`/`legalTargets`/`promotionPending`）の純粋化が次の自然な畝（クリック→着手組み立ての計算部分）。同じ注入パターンで。
- golden snapshot（board-view）への局面追加（第一段a の申し送り）は view 分離の入口で。

---

## 7. 不変の原則（本実装が守るもの）

1. **状態は動かさず、遷移だけ純粋化する**: 参照の広い状態変数（`cursor` 53 箇所等）は board.js に据え置き。移すのは値の計算のみ。blast radius を小さく保つ。
2. **不変**: 純粋関数は新しい `record` を返し、引数を破壊しない。テストの錠が固くなり、第二段b で状態を動かすとき安全。
3. **Wasm 依存は注入点に集約**: `game-record.js` は Wasm を import せず引数で受ける。board.js のラッパが実 Wasm を綴じ、既存の遷移関数名・呼び出し形を保つ。
4. **挙動保存**: `watchAppendTurn` の失敗握り潰し（return）と `branchAndAppend`/`loadPlies` の例外伝播、`phase`/`cursor` の据え置き規則を厳密に保つ。
5. **Rust に触れず Wasm を再ビルドしない**: 純粋 JS ＋テストのみ。配布版据え置き、web `?v=` のみ前進。

---

*第二段a——棋譜コアの遷移を純粋化する。状態変数（`cursor` 53 箇所・`phase` 34 箇所…）は参照が広く、動かせば blast radius が大きい。だから状態は board.js に据え置き、抜くのは遷移＝値の計算だけ。`record = {sfens,events,plies}` を受けて新しい record を返す不変の純粋関数（`appendTurn`/`truncateTo`/`buildFromPlies`）へ寄せ、Wasm は引数注入し、board.js には状態へ橋渡しする薄いラッパ（`setRecord`/`currentRecord`）を残す。実 Wasm を node で注入し、着手適用・分岐切り詰め・不変性・棋譜導出をテストで固める（初手 ☗７六歩 が二つの Wasm を跨いで通ることは地面で確認済み）。branch は cursor で、watch は末尾で切り詰め位置が違うだけ——同じ純粋部品の合成に落ちる。遷移が錠されれば、次に状態を動かすときも安全。*
