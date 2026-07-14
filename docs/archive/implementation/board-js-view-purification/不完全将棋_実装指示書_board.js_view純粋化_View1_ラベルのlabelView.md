# 不完全将棋 実装指示書 — board.js view 純粋化 View-1：ラベルの純粋化（`labelView`）

> 対象実行者: Claude Code（Sonnet 5）
> 前提: 配布 v0.12.3。`render()`（board.js・794 行付近）が `state` から表示値を組み DOM に書いている。この段は**ラベル系の表示値（phaseText/moveText/eventText/archiveInfo/step/total）の導出を純粋関数 `labelView(state, gameOverMsg)` へ抜き**、`render()` はラベル DOM を vm から書くだけにする。ボタンと overlay/cursor は次段（View-2/3）。挙動保存（同じ state → 同じ DOM）。web のみ・`cargo` 不要・`npm test`（vitest）で検証。
> 関連する現物（すべて実地で確認済み・HEAD `c5eb182` 基準）:
> - `web/board.js` `render()`（794-）: `state.phase === 'reveal'` 分岐で `moveText`（`ply.sText　ply.gText`）・`phaseText`（'同時開示'）・`eventText`（`EVENT_LABEL[events[cursor]]`）、`else` 分岐で `phaseText` を watch/online/bothReady/pending/inputStep/gameOver/cursor から組む（800-838）。DOM 書き込み: `phase-label`/`move-display`/`event-label`（846-848）、`archive-info` ＋ `mismatch` class（850-853）、`step-label`（855-857）。
> - 移すヘルパ（いずれも DOM 非依存・現状は module global `state` を読む）: `_watchPhaseText(gameOver)`・`_onlinePhaseText(gameOver)`・`archiveInfoText()`。`formatResult`（`result-view.js`・純粋）を使う。
> - `getGameOverMsg()`（board.js）: `state` を読み `state.gameOverCache` にメモ化し `computeGameOver()`（wasm 経由）を呼ぶ。**この段では `render()` に残し、その結果を `labelView` に引数注入する**（純粋モジュールは wasm を呼ばない）。
> - `EVENT_LABEL`（board.js の表示定数）: `eventText` 用。`navView()`（board.js）: 芽——このアークが育てる `viewModel` の部分ビュー。
> - `web/test/`: vitest。`wasm-loader.js` で wasm を注入するが、`labelView` は wasm 非依存（`gameOverMsg` 注入）なので wasm 不要で走る。
> 関連文書: `不完全将棋_board.js_view純粋化アーク_概観と段組`、`archive/board-split_総括_第零段から第三段b-3`（§3 パターン）。
> 性格: View-1 は**「`render()` のラベル導出（phaseText/moveText/eventText/archiveInfo/step/total）を純粋関数 `labelView(state, gameOverMsg)` へ抜き、ラベル系ヘルパ（watch/online phaseText・archiveInfoText）を state 引数化して同梱する」**。`render()` はラベル DOM を vm から書くだけに。**Wasm は引数注入**（`gameOverMsg`）＝node でテスト可能。ボタン・overlay・SVG・cursor は次段（この段では `render()` に残す）。挙動保存（同じ state → 同じラベル DOM）。web のみ・`?v=` 前進・配布据え置き。

---

## 0. 目的と範囲

- **作るもの**:
  1. `web/view-model.js`（新設）: `labelView(state, gameOverMsg)` ＋ `watchPhaseText(state, gameOver)`・`onlinePhaseText(state, gameOver)`・`archiveInfoText(state)`（state 引数化して移設）。純粋・DOM 非依存・wasm 非依存。
  2. `web/board.js`: `render()` のラベル導出を削り、`labelView` を呼んでラベル DOM を書くだけに。移設したヘルパの旧定義を削除し import へ。`EVENT_LABEL` は view-model.js へ移す（表示定数・board.js で他に使っていれば import で共有）。
  3. `web/test/view-model.test.js`（新設）: `labelView` の golden snapshot テスト。
  4. web `?v=` 前進。
- **位置づけ**: view 純粋化アークの **View-1**。芽 `navView` を `viewModel` へ育てる最初の一歩（ラベル）。
- **作らないもの（＝理由つき）**:
  - **ボタン導出の純粋化**: View-2。この段では `render()` のボタン分岐（859-913）はそのまま。
  - **overlay/cursor/SVG の純粋化**: View-3。`render()` の `parseSfen`/`revealOverlay`/`inputOverlay`/`renderSvg`/カーソル設定はそのまま残す。
  - **`getGameOverMsg` の移設**: wasm＋メモ化なので `render()` に残し、結果を `labelView` へ注入（純粋モジュールは wasm を呼ばない・アーク概観 §1）。
  - **DOM 出力の変更**: 挙動保存。同じ `state`（＋`gameOverMsg`）→ 同じ textContent/class。

---

## 1. `web/view-model.js`（新設・純粋）

```js
import { formatResult } from './result-view.js';

// 事象ラベル（表示定数）。board.js から移設（他で使うなら board.js で import 共有）。
const EVENT_LABEL = { /* board.js の現行定義をそのまま移す */ };

export function watchPhaseText(state, gameOver) {
  // board.js の _watchPhaseText を、module global `state` 参照から引数 `state` へ。ロジック不変。
  // formatResult(state.loadedMeta.result) 等はそのまま。
  …
}

export function onlinePhaseText(state, gameOver) {
  // _onlinePhaseText を state 引数化。ロジック不変。
  …
}

export function archiveInfoText(state) {
  // archiveInfoText を state 引数化。{text, mismatch} を返す。ロジック不変。
  …
}

/**
 * ラベル系の表示値を state（＋盤面から導く終局メッセージ gameOverMsg）から純粋に組む。
 * wasm 非依存（gameOverMsg は呼び出し側が注入）。
 */
export function labelView(state, gameOverMsg) {
  let moveText = '', phaseText = '', eventText = '';

  if (state.phase === 'reveal') {
    const ply = state.plies[state.cursor];
    moveText = `${ply.sText}　${ply.gText}`;
    phaseText = '同時開示';
    const evKey = state.events[state.cursor];
    eventText = (evKey && evKey !== 'normal') ? `（${EVENT_LABEL[evKey] || evKey}）` : '';
  } else {
    const bothReady = !!(state.pendingSente && state.pendingGote);
    if (state.watchMode) {
      phaseText = watchPhaseText(state, gameOverMsg);
    } else if (state.onlineMode) {
      phaseText = onlinePhaseText(state, gameOverMsg);
      if (!state.onlineGameOver && state.onlineCommitted) {
        moveText = state.onlineSide === 'sente' ? (state.pendingSente?.text || '') : (state.pendingGote?.text || '');
      }
    } else if (bothReady) {
      moveText = `${state.pendingSente.text}　${state.pendingGote.text}`;
      phaseText = '解決してください';
    } else if (state.pendingSente) {
      moveText = state.pendingSente.text;
      phaseText = '後手の手を選択中';
    } else if (state.inputStep === 'gote') {
      phaseText = '後手の手を選択中';
    } else if (state.inputStep === 'sente' || state.selectedFrom) {
      phaseText = '先手の手を選択中';
    } else if (gameOverMsg) {
      phaseText = gameOverMsg;
    } else if (state.cursor === 0) {
      phaseText = '初期局面';
    } else {
      phaseText = `第${state.cursor}組手後`;
    }
  }

  const archiveInfo = archiveInfoText(state);
  const total = state.plies.length * 2 + 1;
  const step = state.cursor * 2 + (state.phase === 'reveal' ? 1 : 0) + 1;

  return { phaseText, moveText, eventText, archiveInfo, step, total };
}
```

- **一字一句移す**: 現行 render()／ヘルパのロジックをそのまま。`gameOver` 引数名は `gameOverMsg` に統一（値は同じ）。
- **wasm 非依存**: `getGameOverMsg`/`computeGameOver` は呼ばない。`formatResult`（result-view.js・純粋）のみ。

## 2. `web/board.js` の `render()` を薄く

```js
function render() {
  const pos       = parseSfen(state.sfens[state.cursor]);
  const bothReady = !!(state.pendingSente && state.pendingGote);
  const hasInput  = !!(state.inputStep || state.selectedFrom || state.pendingSente || state.pendingGote);
  const gameOver  = getGameOverMsg();

  // overlay は View-3 まで render に残す（reveal→revealOverlay / else→inputOverlay|null）。
  const overlay = state.phase === 'reveal'
    ? revealOverlay(state.plies[state.cursor])
    : (hasInput ? inputOverlay({ selectedFrom: state.selectedFrom, inputStep: state.inputStep, legalTargets: state.legalTargets }) : null);

  // ラベルは純粋 viewModel から。
  const { phaseText, moveText, eventText, archiveInfo, step, total } = labelView(state, gameOver);

  const svg = document.getElementById('board');
  svg.setAttribute('viewBox', `0 0 ${SVG_W} ${SVG_H}`);
  svg.innerHTML = renderSvg(pos, overlay);
  svg.style.cursor = (state.phase === 'position' && !gameOver && !state.watchMode && !(state.onlineMode && state.onlineCommitted)) ? 'pointer' : 'default';

  document.getElementById('phase-label').textContent  = phaseText;
  document.getElementById('move-display').textContent = moveText;
  document.getElementById('event-label').textContent  = eventText || ' ';

  const archiveInfoEl = document.getElementById('archive-info');
  archiveInfoEl.textContent = archiveInfo.text;
  archiveInfoEl.classList.toggle('mismatch', archiveInfo.mismatch);

  document.getElementById('step-label').textContent = `${step} / ${total}`;

  // …ボタン分岐（859-913）は View-2 までそのまま…
}
```

- `render()` から: reveal/else の**ラベル導出ブロック**（800-838）と `archiveInfoText()` 呼び出し・step/total 計算を削り、`labelView` の分割代入に置換。
- **削除**: board.js の `_watchPhaseText`/`_onlinePhaseText`/`archiveInfoText`/`EVENT_LABEL` の旧定義（view-model.js へ移設）。`import { labelView } from './view-model.js';` を足す。他所で `EVENT_LABEL` を使っていれば view-model.js から import 共有。
- `bothReady`/`hasInput` は overlay とボタン（View-2 まで）で使うので render に残す。

## 3. テスト（`web/test/view-model.test.js`・golden snapshot）

`labelView(state, gameOverMsg)` の返り値を代表的な state で固定する。wasm 不要（gameOverMsg 注入）。

- reveal 局面（sText/gText・event あり/なし）。
- 観戦: `watchStatusText` = connecting/error/closed/player_disconnected（concluded 有無）/通常（開始待ち・最新・第N組手）。
- オンライン: waiting（`onlineWaitingMsg`）・committed（moveText が自陣営の pending）・gameOver（`onlineEndMsg`・初期/第N）。
- ローカル: bothReady（'解決してください'）・pendingSente のみ・inputStep gote/sente・selectedFrom・gameOver（gameOverMsg 注入）・cursor 0（初期局面）・cursor 途中（第N組手後）。
- `archiveInfo`: loadedMeta なし（空）・あり（version/result 行）・mismatch（ルール不一致の警告）。
- `step`/`total`: reveal と position で step が 1 ずれること。

## 4. 受け入れ条件

- `web/view-model.js` に `labelView`＋3 ヘルパ（state 引数化）があり、wasm・DOM 非依存。
- `render()` がラベル導出を `labelView` へ委譲し、ラベル DOM を vm から書く。旧ヘルパ定義が board.js から消え import に。
- **DOM 出力がバイト単位で保存**（同じ state・gameOver → 同じ textContent/class）。ブラウザで各局面（reveal/観戦/オンライン/ローカル/gameOver/棋譜再生）を目視し、phase-label・move-display・event-label・archive-info・step-label が従来と同一。
- `npm test`（vitest）緑（新規 view-model テスト＋既存無傷）。
- board.js のボタン・overlay・SVG は無変更。engine/protocol/tui/server に差分なし。web `?v=` 前進・配布据え置き。

## 末尾要約

`render()` のラベル導出（phaseText/moveText/eventText/archiveInfo/step/total）を純粋関数 `labelView(state, gameOverMsg)` へ抜き、`web/view-model.js` を新設する。ラベル系ヘルパ（`watchPhaseText`/`onlinePhaseText`/`archiveInfoText`）を state 引数化して同梱し、`EVENT_LABEL` も移す。盤面から導く終局メッセージは `getGameOverMsg`（wasm＋メモ化）を render に残して**結果を引数注入**——純粋モジュールは wasm を呼ばず node でテストできる。`render()` はラベル DOM を vm から書くだけに。ボタン・overlay・SVG は次段。golden snapshot で「同じ state → 同じラベル DOM」を守る。挙動保存・web `?v=` 前進・配布据え置き。芽 `navView` を `viewModel` へ育てる最初の一歩。

## 不変の原則

- **表示値は純粋・描画は薄い殻**: `labelView(state, gameOverMsg)` がラベルを組み、`render()` は DOM へ流すだけ。
- **Wasm は引数注入**: `labelView` は wasm を呼ばない（`gameOverMsg` を受ける）。node でテスト可能・ビルドレス維持。
- **挙動保存**: ラベル DOM（textContent・mismatch class）を保存。golden snapshot で守る。ロジックは一字一句移す。
- **芽を育てる**: `navView` の延長として `labelView` を置く（新しい平行構造を作らない）。次段でボタン・overlay を同じ viewModel へ寄せる。
- **この段はラベルだけ**: ボタン・overlay・SVG・cursor・`getGameOverMsg` は render に残す。触るのは view-model.js（新設）と board.js のラベル部のみ。
