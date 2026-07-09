# 不完全将棋 実装指示書 — 第三段b-3：オンラインリセットとホットシート確定を純粋 reduce へ（書き込み集約の完了・分割アークの区切り）

> 対象実行者: Claude Code（Sonnet 5 または Haiku 4.5）
> 前提: 配布 v0.11.2 / web `?v=`0.11.10（board.js 分割 第三段b-2 まで着地。状態は単一の `state`、更新は `update(patch)` を通り、局面ナビは純粋 `navReduce`〔`nav.js`〕へ抜けた。web テスト 53 件・golden snapshot・純粋モジュール 9 本が据わっている。board.js は 1234 行）。
> 関連する現物（すべて実地で確認済み）:
> - **種類1（純粋な状態遷移）の残りは二つ**。b-2 で確立した「純粋 reduce＋board.js ラッパ」の鋳型がそのまま効く。
> - `_resetOnlineState`（488–）は **11 変数を定数へ戻すだけ**の純粋なリセット。呼び出し元は 6 箇所（300・345・1004・1023・1110）で、いずれも `disconnectOnline()` か `resetToNew()` という**別の I/O・遷移と組で**呼ばれる——`_resetOnlineState` 自体に I/O は無い。戻す値は初期値そのもの（`resultOverride` 等は 558 の投了で別途設定されるが、リセットは初期化の責務のみ。境界は綺麗）。
> - `confirmMove`（460–）の**ホットシート分岐**は純粋な状態遷移（先手確定なら `pendingSente` セット＋`inputStep:'gote'`、後手確定なら `pendingGote` セット）。**オンライン分岐**は `commitMoveOnline`（I/O）を含むので純粋化しない。両分岐とも `usiToText`（Wasm 糊）で text を作り、`hidePromotionUI`（DOM）を呼ぶ——これらは reduce の外。呼び出し元は 456（`selectTarget`）・970・975。
> - **相似形の北極星（tui）**: `tui/src/app.rs` の `new_game`（リセット相当）・`confirm_move`（確定相当）に対応。tui は中間形態ゆえ、web はより純粋な形（patch を返す reduce）へ進む。
> 関連文書: `不完全将棋_実装指示書_局面ナビゲーションの遷移を純粋reduceへ_board分割第三段b-2`（鋳型）、`不完全将棋_実装指示書_kifu吸収と状態更新経路_board分割第三段b-1`（`update` 経路）、`不完全将棋_バックログ_伏線と未決`。
> 性格: 第三段b-3 は**「種類1 の残り（オンラインリセット・ホットシート確定）を純粋 reduce へ抜き、書き込み集約を完了させる。board.js 分割アークの区切り」**。b-2 と同じ「純粋 reduce（値を受け patch を返す）＋board.js ラッパ（I/O・DOM を残す）」で統一。これで種類1 の純粋遷移が `navReduce`＋`resetOnlineReduce`＋`confirmReduce` と出揃い、書き込み集約が閉じる。**種類2（I/O 絡み）と view 純粋化は、地面を測り直してから次のアークで**（急がば回れ・過ぎたるは及ばざる）。Rust に触れず Wasm 再ビルドなし。製品挙動は不変。行番号は v0.11.10 基準。

---

## 0. 目的と範囲

- **作るもの**:
  1. `web/reducers.js` — 種類1 の純粋遷移を集める。`resetOnlineReduce()`（オンライン関連 11 変数を初期値へ戻す patch を返す）と `hotseatConfirmReduce(side)`（ホットシート確定の状態遷移 patch を返す）。純粋（値を受け値を返す、DOM も Wasm も状態も触れない）。
  2. `web/test/reducers.test.js` — 両 reduce を検証（Wasm 不要）。
  3. board.js の `_resetOnlineState`・`confirmMove` を、reduce を呼んで patch を適用する形に整理（I/O・DOM・Wasm 糊は残す）。
  - `nav.js` を `reducers.js` へ統合するかは任意（§1 の注記参照）。本書は `reducers.js` を新設し `navReduce` はそのまま `nav.js` に残す（統合は不要な移動を生むので避ける）。
- **位置づけ**: board.js 分割の**第三段b-3**。書き込み集約の完了。種類1 の純粋遷移が出揃い、状態更新が「純粋 reduce で patch を計算 → `update` で適用」に統一される。**この分割アークの区切り**。
- **作らないもの（＝理由つき）**:
  - **種類2（I/O 絡み）の分解**（`handleTurnComplete`・`enterWatchMode`・`endOnlineGame`）: 非同期コールバックと絡み、設計の自由度が大きい。tui の online.rs に倣う分離は、地面を測り直して**次のアーク**で。
  - **view 純粋化**（render() 本体）: DOM 書き込み 20 箇所超の巨大関数。次のアーク。
  - **統一ディスパッチャ／action 型の導入**: 種類1 が 3 つの小さな純粋関数で足りている。汎化は必要が呼んでから（過ぎたるは及ばざる）。
  - **`confirmMove` のオンライン分岐の純粋化**: `commitMoveOnline`（WS I/O）を含む。ホットシート分岐のみ純粋化。

---

## 1. `web/reducers.js`（純粋・DOM も Wasm も状態も触れない）

```js
// 状態遷移の純粋 reduce 群（種類1）。値を受け patch（変化分）を返す。
// DOM・Wasm・可変状態に非依存。board.js 分割 第三段b-3。
// （局面ナビの navReduce は nav.js に別置。将来ここへ集約する余地はあるが本書では移さない。）

// オンライン関連の状態を初期値へ戻す patch。対局終了・退出・新規対局で使う。
export function resetOnlineReduce() {
  return {
    onlineMode: false,
    onlineSide: null,
    onlineGameOver: false,
    onlineEndMsg: '',
    onlineCommitted: false,
    onlineWaiting: false,
    onlineWaitingMsg: '',
    resultOverride: null,
    recordInviteAsked: false,
    recordStatusText: '',
    archivedLink: null,
    _pendingRecordDisconnect: false,
  };
}

// ホットシート（同一端末で両者指す）モードの確定後の状態遷移 patch。
// side==='sente' なら後手入力へ進む。text は呼び出し側が usiToText で作って渡す。
//   pending = { usi, text }
export function hotseatConfirmReduce(side, pending) {
  if (side === 'sente') {
    return { pendingSente: pending, inputStep: 'gote', selectedFrom: null, legalTargets: null, promotionPending: null };
  }
  return { pendingGote: pending, selectedFrom: null, legalTargets: null, promotionPending: null };
}
```

- `resetOnlineReduce` は現 `_resetOnlineState` の 11 代入と**同じ初期値**（現物から一字一句移す）。
- `hotseatConfirmReduce` は現 `confirmMove` のホットシート分岐と同じ結果（先手確定→`inputStep:'gote'`＋pending、後手確定→pending のみ）＋ 選択/promotion クリア（元は別 `update` で行っていたものをまとめる。§2 参照）。

## 2. board.js 側の整理

`reducers.js` を import:

```js
import { resetOnlineReduce, hotseatConfirmReduce } from './reducers.js';
```

### `_resetOnlineState`

```js
function _resetOnlineState() {
  Object.assign(state, resetOnlineReduce());
}
```

- **注意**: 元の `_resetOnlineState` は `render()` を**呼ばない**（呼び出し元が `disconnectOnline()` や `resetToNew()` の後で描画する）。よってここは `update(...)` ではなく `Object.assign(state, ...)` を使い、**描画を足さない**（挙動保存）。呼び出し元 6 箇所は無変更。

### `confirmMove`（ホットシート分岐のみ reduce 化）

```js
function confirmMove(usi) {
  const side = state.inputStep === 'gote' ? 'gote' : 'sente';
  const text = usiToText(usi, state.sfens[state.cursor], side);   // Wasm 糊：残す

  if (state.onlineMode) {
    // オンライン分岐：commitMoveOnline（I/O）を含むので純粋化しない（従来通り）
    if (side === 'sente') state.pendingSente = { usi, text };
    else                  state.pendingGote  = { usi, text };
    hidePromotionUI();
    commitMoveOnline(state.sfens[state.cursor], usi);
    update({ inputStep: null, selectedFrom: null, legalTargets: null, promotionPending: null, onlineCommitted: true });
    return;
  }

  // ホットシート分岐：純粋遷移へ委譲
  hidePromotionUI();                                    // DOM：残す
  update(hotseatConfirmReduce(side, { usi, text }));
}
```

- **挙動保存の確認**: 元は「`state.pendingSente = ...; state.inputStep = 'gote'; hidePromotionUI(); update({ selectedFrom:null, legalTargets:null, promotionPending:null })`」。新は `hotseatConfirmReduce` が pending・inputStep・選択クリア・promotion クリアを**一つの patch にまとめ**、`update` で一度に適用する。結果の状態は同一（`pendingSente`/`inputStep`/`selectedFrom`/`legalTargets`/`promotionPending` すべて元と同じ値）。`hidePromotionUI()` は DOM 副作用として update の前に残す。描画回数は元と同じ（1 回）。
- オンライン分岐は無変更（`onlineCommitted` の設定含め従来通り）。

## 3. テスト `web/test/reducers.test.js`（Wasm 不要）

```js
import { describe, it, expect } from "vitest";
import { resetOnlineReduce, hotseatConfirmReduce } from "../reducers.js";

describe("resetOnlineReduce（オンライン状態の初期化）", () => {
  it("11 のオンライン関連キーをすべて初期値へ戻す", () => {
    const p = resetOnlineReduce();
    expect(p).toEqual({
      onlineMode: false, onlineSide: null, onlineGameOver: false, onlineEndMsg: '',
      onlineCommitted: false, onlineWaiting: false, onlineWaitingMsg: '',
      resultOverride: null, recordInviteAsked: false, recordStatusText: '',
      archivedLink: null, _pendingRecordDisconnect: false,
    });
  });
  it("呼ぶたびに独立した新しいオブジェクトを返す（共有しない）", () => {
    expect(resetOnlineReduce()).not.toBe(resetOnlineReduce());
  });
});

describe("hotseatConfirmReduce（ホットシート確定の遷移）", () => {
  it("先手確定：pendingSente をセットし後手入力へ進む＋選択/成りクリア", () => {
    const p = hotseatConfirmReduce('sente', { usi: '7g7f', text: '☗７六歩' });
    expect(p).toEqual({
      pendingSente: { usi: '7g7f', text: '☗７六歩' },
      inputStep: 'gote', selectedFrom: null, legalTargets: null, promotionPending: null,
    });
  });
  it("後手確定：pendingGote をセット（inputStep は進めない）＋選択/成りクリア", () => {
    const p = hotseatConfirmReduce('gote', { usi: '3c3d', text: '☖３四歩' });
    expect(p).toEqual({
      pendingGote: { usi: '3c3d', text: '☖３四歩' },
      selectedFrom: null, legalTargets: null, promotionPending: null,
    });
    expect('inputStep' in p).toBe(false);  // 後手確定は inputStep を触らない
  });
});
```

## 4. 受け入れ

- `cd web && npm test` が緑（既存 53 件＋新規 `reducers` 4 件、warn なし。snapshot 差分ゼロ）。
- ブラウザで従来通り: ホットシートで先手確定→後手入力へ、後手確定→両者着手済み（→で解決）、成り選択後の確定、オンラインで確定→commit 送信（従来通り）、対局終了・退出・新規対局でのオンライン状態リセット（切断・盤リセットと組で）。
- **特に確認**: `_resetOnlineState` が描画を足さない（呼び出し元の描画に委ねる）こと、ホットシート確定後に `inputStep` が正しく（先手→'gote'、後手→変えず）遷移すること、オンライン分岐が無変更で動くこと。

## 5. 版の刻み

- **製品挙動は不変・Rust 非関与・Wasm 再ビルドなし**。第三段の他段と同じ扱い: 配布版据え置き **v0.11.2**、web の `?v=`（`web/package.json`・`web/index.html`）を **0.11.11** へ前進（board.js が `reducers.js` を新規 import するためキャッシュ更新）。**RULE 0.6・PROTOCOL 4・アーカイブ書式 1 不変**。

## 6. 申し送り（次のアークへ——地面を測り直してから）

- **書き込み集約が完了**した。種類1 の純粋遷移が `navReduce`（nav.js）＋`resetOnlineReduce`＋`hotseatConfirmReduce`（reducers.js）と出揃い、状態更新は「純粋 reduce で patch → `update` で適用」に統一。この分割アークはここで区切る。
- **次のアーク（重い・設計相談から）**:
  - **種類2（I/O 絡みの分解）**: `handleTurnComplete`（online＋メタ＋棋譜）・`enterWatchMode`（4 島・非同期コールバック）・`endOnlineGame`（setTimeout・記録係綴じ待ち）。tui の online.rs に倣い「純粋な状態遷移」と「I/O」に割る。純粋部分は reduce へ、I/O は殻に残す。**非同期コールバックの状態更新をどう純粋遷移として括り出すか**が設計の核。地面（各コールバックの発火経路）を測り直してから。
  - **view 純粋化**: render() 本体（DOM 書き込み 20 箇所超）の phaseText/ボタン分岐を「`state` スナップショット→表示値」の純粋関数へ。`navView()` の「state から必要部分を切り出す」パターンが芽。集約が済んだ今、「同じ `state` → 同じ描画」をテストで守れる地盤はある。
  - **golden snapshot への局面追加**（第一段a の申し送り）は view 段で。
  - `nav.js` と `reducers.js` の統合は、種類2 の reduce が増えて「遷移モジュールが散る」と感じたら検討（今は不要）。

---

## 7. 不変の原則（本実装が守るもの）

1. **純粋 reduce で統一**: `resetOnlineReduce()`・`hotseatConfirmReduce(side, pending)` は値を受け patch を返す（nav・game-record・move-input・overlay と同じ流儀）。
2. **副作用は reduce の外**: I/O（`disconnectOnline`/`commitMoveOnline`）・DOM（`hidePromotionUI`）・Wasm 糊（`usiToText`）は board.js のラッパに残す。
3. **描画の有無を保存**: `_resetOnlineState` は描画を足さない（`Object.assign`）。`confirmMove` は従来通り 1 回描画（`update`）。挙動を一字も変えない。
4. **区切りを綺麗に**: 種類1 を出揃わせて書き込み集約を閉じる。種類2・view は次のアークへ。急がば回れ、過ぎたるは及ばざる。
5. **Rust に触れず Wasm を再ビルドしない**: 純粋 JS ＋テストのみ。配布版据え置き、web `?v=` のみ前進。

---

*第三段b-3——オンラインリセットとホットシート確定を純粋 reduce へ。書き込み集約の完了、そして board.js 分割アークの区切り。b-2 で確立した鋳型で、種類1 の残り二つ（11 変数を初期値へ戻す `resetOnlineReduce`、ホットシート確定の遷移 `hotseatConfirmReduce`）を `reducers.js` へ抜く。I/O（disconnect/commit）・DOM（promotion）・Wasm 糊（usiToText）は board.js のラッパに残し、純粋遷移だけを括り出す。`_resetOnlineState` は描画を足さない挙動を保存（Object.assign）、`confirmMove` は従来通り 1 回描画。これで種類1 が navReduce＋resetOnlineReduce＋hotseatConfirmReduce と出揃い、状態更新が「純粋 reduce で patch → update で適用」に統一される——書き込み集約が閉じる。種類2（I/O 絡み）と view 純粋化は重く、地面を測り直して次のアークで。急がず、区切り良く、本丸の主要部を綺麗に畳む。*
