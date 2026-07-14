# 不完全将棋 実装指示書 — board.js view 純粋化 View-3：overlay/cursor の純粋化と `viewModel` の合成（本丸の締め）

> 対象実行者: Claude Code（Sonnet 5）
> 前提: View-2 着地（HEAD `a2d3804`。`web/view-model.js` に純粋 `labelView`・`buttonView`、`render()` はラベルとボタンを委譲）。この段は**残る overlay と cursor 判定を純粋関数へ抜き、`viewModel(state, gameOverMsg)` へ合成する**。`render()` は「wasm 依存の pos・gameOver を作り、pure な `viewModel` を一度呼び、DOM に流すだけ」の薄い殻に育ち切る。**これで view 純粋化アークが綴じる**。挙動保存（同じ state → 同じ描画）。web のみ・`npm test`（vitest）で検証。
> 関連する現物（すべて実地で確認済み・HEAD `a2d3804` 基準）:
> - `web/board.js` `render()` 冒頭（758-773）:
>   - `pos = parseSfen(state.sfens[state.cursor])`。**`parseSfen` は `wasmPositionView(sfen)` を呼ぶ＝wasm 依存**（`positionViewToState(JSON.parse(wasmPositionView(sfen)))`）。→ **純粋モジュールに入れず render に残す**。
>   - `overlay = state.phase === 'reveal' ? revealOverlay(state.plies[state.cursor]) : (hasInput ? inputOverlay({selectedFrom, inputStep, legalTargets}) : null)`。`hasInput = !!(inputStep || selectedFrom || pendingSente || pendingGote)`。
>   - `svg.setAttribute('viewBox', …)`・`svg.innerHTML = renderSvg(pos, overlay)`・`svg.style.cursor = (state.phase==='position' && !gameOver && !watchMode && !(onlineMode && onlineCommitted)) ? 'pointer' : 'default'`。
>   - `gameOver = getGameOverMsg()`（wasm＋メモ化・render に残す）。
> - `revealOverlay`/`inputOverlay`/`renderSvg` は `board-view.js`（**wasm 非依存の純粋モジュール**・総括 §1）。`view-model.js` から `revealOverlay`/`inputOverlay` を import してよい（純粋のまま）。`renderSvg` は DOM 直前なので render に残す。
> - `web/view-model.js`（View-1/2）: `import { formatResult } from './result-view.js'` のみ。`labelView`・`buttonView`（純粋）。ここに `overlay`・`cursorInteractive`・合成 `viewModel` を足す。
> - `navView()`（board.js）: nav.js の `navReduce` の入力スナップショット。**別用途なので触らない**（render の `viewModel` とは消費者が違う）。
> 関連文書: `不完全将棋_board.js_view純粋化アーク_概観と段組`、View-1/2 指示書。
> 性格: View-3 は**「render() の overlay と cursor 判定を純粋関数へ抜き、`labelView`＋`buttonView`＋`overlay`＋`cursorInteractive` を合成した `viewModel(state, gameOverMsg)` を置く」**。`render()` は wasm 依存の `pos`・`gameOver` を作って `viewModel` を一度呼び、DOM に流すだけの薄い殻に。**Wasm は引数注入**（pos・gameOver を render で作り、pure な viewModel には持ち込まない）。挙動保存。web のみ・`?v=` 前進・配布据え置き。**このアークの締め**。

---

## 0. 目的と範囲

- **作るもの**:
  1. `web/view-model.js`: `overlay(state)`・`cursorInteractive(state, gameOverMsg)`・合成 `viewModel(state, gameOverMsg)` を追加。`revealOverlay`/`inputOverlay` を board-view.js から import（純粋維持）。
  2. `web/board.js`: `render()` を「pos（wasm）・gameOver（wasm）を作る → `viewModel` を呼ぶ → DOM に流す」の薄い殻に。overlay/cursor のインライン導出を削除。
  3. `web/test/view-model.test.js`: `overlay`・`cursorInteractive`（と合成 `viewModel`）の golden snapshot を追加。
  4. web `?v=` 前進。
- **位置づけ**: view 純粋化アークの **View-3（締め）**。芽 `navView` の precedent が、render 用には完全な `viewModel` へ育ち切る。
- **作らないもの（＝理由つき）**:
  - **`pos`（parseSfen）を viewModel へ入れる**: `parseSfen` は wasm 依存（`wasmPositionView`）。純粋モジュールは wasm を呼ばない原則により、render に残して `renderSvg(pos, vm.overlay)` へ注入する。
  - **`renderSvg`/`getGameOverMsg` の移設**: DOM 直前／wasm なので render に残す。
  - **`navView` の変更**: nav.js の `navReduce` 入力。別消費者なので無変更（render の viewModel とは別）。
  - **描画出力の変更**: 挙動保存（同じ state → 同じ SVG・cursor・ラベル・ボタン DOM）。

---

## 1. `web/view-model.js` に overlay/cursor/viewModel を追加

```js
import { formatResult } from './result-view.js';
import { inputOverlay, revealOverlay } from './board-view.js';   // 純粋（wasm 非依存）

// …（既存 EVENT_LABEL・watchPhaseText・onlinePhaseText・archiveInfoText・labelView・buttonView）…

/** overlay（reveal→開示 overlay／入力中→入力 overlay／それ以外→null）。純粋。 */
export function overlay(state) {
  if (state.phase === 'reveal') {
    return revealOverlay(state.plies[state.cursor]);
  }
  const hasInput = !!(state.inputStep || state.selectedFrom || state.pendingSente || state.pendingGote);
  return hasInput
    ? inputOverlay({ selectedFrom: state.selectedFrom, inputStep: state.inputStep, legalTargets: state.legalTargets })
    : null;
}

/** 盤の SVG カーソルがポインタ（操作可能）か。純粋。 */
export function cursorInteractive(state, gameOverMsg) {
  return state.phase === 'position'
    && !gameOverMsg
    && !state.watchMode
    && !(state.onlineMode && state.onlineCommitted);
}

/** 描画に必要な表示値を一つの束に合成する（pos・gameOverMsg は wasm 依存なので呼び出し側が用意）。純粋。 */
export function viewModel(state, gameOverMsg) {
  return {
    ...labelView(state, gameOverMsg),         // phaseText, moveText, eventText, archiveInfo, step, total
    buttons: buttonView(state, gameOverMsg),  // next, prev, resign, save, startButtonsDisabled, leaveWatchHidden
    overlay: overlay(state),
    cursorInteractive: cursorInteractive(state, gameOverMsg),
  };
}
```

- **一字一句移す**: overlay/cursor の条件は現行 render() のまま。
- **純粋維持**: board-view.js は wasm 非依存なので import しても view-model.js は wasm フリー（node でテスト可能・ビルドレス維持）。

## 2. `web/board.js` の `render()` を薄い殻に

```js
function render() {
  const pos      = parseSfen(state.sfens[state.cursor]);   // wasm（注入用）
  const gameOver = getGameOverMsg();                        // wasm＋メモ化（注入用）
  const vm       = viewModel(state, gameOver);              // 純粋な表示値の束

  const svg = document.getElementById('board');
  svg.setAttribute('viewBox', `0 0 ${SVG_W} ${SVG_H}`);
  svg.innerHTML = renderSvg(pos, vm.overlay);
  svg.style.cursor = vm.cursorInteractive ? 'pointer' : 'default';

  document.getElementById('phase-label').textContent  = vm.phaseText;
  document.getElementById('move-display').textContent = vm.moveText;
  document.getElementById('event-label').textContent  = vm.eventText || ' ';

  const archiveInfoEl = document.getElementById('archive-info');
  archiveInfoEl.textContent = vm.archiveInfo.text;
  archiveInfoEl.classList.toggle('mismatch', vm.archiveInfo.mismatch);

  document.getElementById('step-label').textContent = `${vm.step} / ${vm.total}`;

  const b = vm.buttons;
  const btnNext = document.getElementById('btn-next');
  btnNext.textContent = b.next.text;
  btnNext.disabled    = b.next.disabled;
  document.getElementById('btn-prev').disabled = b.prev.disabled;

  const btnResign = document.getElementById('btn-resign');
  if (btnResign) {
    btnResign.style.display = b.resign.visible ? 'inline-block' : 'none';
    btnResign.disabled      = b.resign.disabled;
  }
  const btnSave = document.getElementById('btn-save');
  if (btnSave) btnSave.classList.toggle('highlight', b.save.highlight);
  for (const id of ['btn-online', 'btn-load']) {
    const el = document.getElementById(id);
    if (el) el.disabled = b.startButtonsDisabled;
  }
  const btnLeaveWatch = document.getElementById('btn-leave-watch');
  if (btnLeaveWatch) btnLeaveWatch.hidden = b.leaveWatchHidden;
}
```

- **削除**: `overlay` のインライン導出（762-766）と、View-1/2 が残していた `bothReady`/`hasInput` の render 冒頭計算（overlay も buttonView も内部で持つので不要）。cursor の長い条件式は `vm.cursorInteractive` に。
- **import 更新**: `import { labelView, buttonView, viewModel } from './view-model.js';`（labelView/buttonView を render が直接呼ばなくなるなら `viewModel` だけの import に整理してよい。ただし他所で labelView/buttonView を使っていなければ、render は `viewModel` 一本で足りる）。
- **残す**: `parseSfen`（wasm）・`getGameOverMsg`（wasm）・`renderSvg`（DOM 直前）。これらが「殻が wasm 結果を注入する」境界。

## 3. テスト（`web/test/view-model.test.js` に追加）

- `overlay(state)`: reveal（`revealOverlay` の結果）・入力中（selectedFrom/inputStep/legalTargets で `inputOverlay`）・入力なし（null）。
- `cursorInteractive(state, gameOverMsg)`: position×非終局×非観戦×非コミット → true。gameOver あり／watchMode／online&committed → false。reveal → false。
- 合成 `viewModel(state, gameOverMsg)`: 代表 state で、`phaseText`/`buttons`/`overlay`/`cursorInteractive` が labelView/buttonView/overlay/cursorInteractive 個別呼びと一致する（合成の透過性）。
- wasm 不要（overlay は board-view.js の純粋関数・gameOverMsg 注入）。

## 4. 受け入れ条件

- `web/view-model.js` に `overlay`・`cursorInteractive`・`viewModel`（いずれも純粋・DOM/wasm 非依存）。board-view.js の import で view-model.js が wasm フリーを維持。
- `render()` が「pos・gameOver（wasm）を作る → `viewModel` を呼ぶ → DOM に流す」の薄い殻。overlay/cursor のインライン導出が消えている。
- **描画出力がバイト単位で保存**（同じ state → 同じ SVG innerHTML・cursor・ラベル・ボタン）。ブラウザで各局面（reveal/入力中/観戦/オンライン/棋譜再生/終局）を目視。
- `npm test`（vitest）緑（overlay/cursorInteractive/viewModel テスト追加＋既存無傷）。
- engine/protocol/tui/server・`navView` に差分なし。web `?v=` 前進・配布据え置き。

## 5. アークの締め

View-3 着地で **view 純粋化アークが綴じる**。`render()` は「wasm 結果を注入して pure な `viewModel` を作り、DOM に流す薄い殻」になり、表示値の導出はすべて `web/view-model.js`（純粋・node テスト可能）に集まる。これで**ルール変更で終局種別や phaseText が増えても、`view-model.js` に局面を足して golden snapshot で守れる**地盤ができた。総括（`archive/` 行き）を綴じ、バックログ §D から「view 純粋化」を落とす。残る board.js 本丸は「種類2＝I/O 分解（頑健性向上を畳み込む）」——脅威の切迫度が実感で決まったら別アークとして。

## 末尾要約

`render()` の overlay と cursor 判定を純粋関数 `overlay(state)`・`cursorInteractive(state, gameOverMsg)` へ抜き、`labelView`＋`buttonView`＋overlay＋cursor を合成した `viewModel(state, gameOverMsg)` を置く。`render()` は wasm 依存の `pos`（parseSfen）・`gameOver`（getGameOverMsg）を作り、pure な `viewModel` を一度呼び、DOM に流すだけの薄い殻になる。`parseSfen` は wasm 依存なので純粋モジュールに入れず注入する。board-view.js（wasm 非依存）から overlay 関数を import しても view-model.js は node でテスト可能。golden snapshot で「同じ state → 同じ描画」を守る。挙動保存・web `?v=` 前進・配布据え置き。**view 純粋化アークの締め**——表示値の導出が純粋モジュールに集まり、ルール変更の表示追加が安全でテスト可能になる。

## 不変の原則

- **描画は薄い殻・表示値は純粋**: `render()` は wasm 結果を注入して `viewModel` を作り DOM に流すだけ。導出は `view-model.js` に集約。
- **Wasm は引数注入**: `pos`・`gameOver` は render（殻）で作り、pure な `viewModel` に持ち込まない。view-model.js は wasm フリー・node テスト可能・ビルドレス維持。
- **挙動保存**: SVG・cursor・ラベル・ボタンの DOM 出力を保存。golden snapshot で守る。条件は一字一句移す。
- **芽を育て切る**: render 用の `viewModel` が完成形。`navView`（nav.js の別消費者）は触らない。
- **この段で締める**: overlay/cursor が最後。種類2（I/O 分解）は別アーク。触るのは view-model.js（追加）と board.js の render のみ。
