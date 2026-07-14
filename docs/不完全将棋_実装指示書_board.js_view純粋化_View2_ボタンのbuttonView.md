# 不完全将棋 実装指示書 — board.js view 純粋化 View-2：ボタンの純粋化（`buttonView`）

> 対象実行者: Claude Code（Sonnet 5）
> 前提: View-1 着地（HEAD `ae7f3cc`。`web/view-model.js` に純粋 `labelView`＋ラベルヘルパ、`render()` はラベルを委譲）。この段は**ボタンの表示状態（次/前/投了/保存/観戦離脱/対局開始系の text・disabled・visible）の導出を純粋関数 `buttonView(state, gameOverMsg)` へ抜き**、`render()` はボタン DOM を vm から書くだけにする。overlay/cursor/SVG は次段（View-3）。挙動保存（同じ state → 同じボタン DOM）。web のみ・`npm test`（vitest）で検証。
> 関連する現物（すべて実地で確認済み・HEAD `ae7f3cc` 基準）:
> - `web/board.js` `render()` のボタン節（787-841）:
>   - `btn-next`/`btn-prev`（787-822）: watch（'次 →'・`disabled = !(reveal || (position && cursor<plies.length))`／prev `cursor===0 && position`）／online（gameOver なら watch と同じナビ、非 gameOver は両方 disabled）／ローカル（`text = bothReady?'解決 →':'次 →'`・`disabled = !(bothReady || reveal || (position && !hasInput && cursor<plies.length))`／prev `cursor===0 && position && !hasInput && !promotionPending`）。
>   - `btn-resign`（823-826）: `display = (onlineMode && !onlineGameOver)?'inline-block':'none'`・`disabled = onlineCommitted || onlineWaiting`。
>   - `btn-save`（829-832）: `highlight = isOver`（`isOver = onlineMode ? onlineGameOver : !!gameOver`）。
>   - `btn-online`/`btn-load`（836-838）: `disabled = watchMode`。
>   - `btn-leave-watch`（840-841）: `hidden = !watchMode`。
>   - 依存フラグ（純粋・state から）: `bothReady = !!(pendingSente && pendingGote)`・`hasInput = !!(inputStep || selectedFrom || pendingSente || pendingGote)`・`gameOver = getGameOverMsg()`（render に残す・結果を注入）。
> - `web/view-model.js`（View-1）: `labelView(state, gameOverMsg)`＋ヘルパ。純粋・DOM/wasm 非依存。ここに `buttonView` を足す。
> - `web/test/view-model.test.js`: vitest（golden snapshot）。
> 関連文書: `不完全将棋_board.js_view純粋化アーク_概観と段組`、View-1 指示書。
> 性格: View-2 は**「`render()` のボタン導出（next/prev/resign/save/観戦離脱/対局開始系の text・disabled・visible）を純粋関数 `buttonView(state, gameOverMsg)` へ抜く」**。`render()` はボタン DOM を vm から書くだけに。**Wasm は引数注入**（`gameOverMsg`）。overlay/SVG/cursor は次段（この段では render に残す）。挙動保存（同じ state → 同じボタン DOM）。web のみ・`?v=` 前進・配布据え置き。

---

## 0. 目的と範囲

- **作るもの**:
  1. `web/view-model.js`: `buttonView(state, gameOverMsg) → { next{text,disabled}, prev{disabled}, resign{visible,disabled}, save{highlight}, startButtonsDisabled, leaveWatchHidden }` を追加。純粋・DOM/wasm 非依存。
  2. `web/board.js`: `render()` のボタン節（787-841）を `buttonView` の結果から DOM を書くだけに。
  3. `web/test/view-model.test.js`: `buttonView` の golden snapshot テストを追加。
  4. web `?v=` 前進。
- **位置づけ**: view 純粋化アークの **View-2**。芽 `navView`→`viewModel` の育成の二歩目（ボタン）。
- **作らないもの（＝理由つき）**:
  - **overlay/cursor/SVG の純粋化**: View-3。`render()` の `parseSfen`/`revealOverlay`/`inputOverlay`/`renderSvg`/カーソル設定はそのまま。
  - **`getGameOverMsg` の移設**: render に残し結果を注入（純粋モジュールは wasm を呼ばない）。
  - **ボタン DOM 出力の変更**: 挙動保存（同じ state・gameOver → 同じ text/disabled/display/hidden/class）。
  - **`labelView` の変更**: View-1 のまま。

---

## 1. `web/view-model.js` に `buttonView` を追加

```js
/**
 * ボタンの表示状態を state（＋終局メッセージ gameOverMsg）から純粋に組む。
 * wasm 非依存（gameOverMsg 注入）。ロジックは現行 render() のボタン節を一字一句移す。
 */
export function buttonView(state, gameOverMsg) {
  const bothReady = !!(state.pendingSente && state.pendingGote);
  const hasInput  = !!(state.inputStep || state.selectedFrom || state.pendingSente || state.pendingGote);
  const atStart   = state.cursor === 0 && state.phase === 'position';
  const canForward = state.phase === 'reveal' || (state.phase === 'position' && state.cursor < state.plies.length);

  let next, prev;
  if (state.watchMode) {
    next = { text: '次 →', disabled: !canForward };
    prev = { disabled: atStart };
  } else if (state.onlineMode) {
    if (state.onlineGameOver) {
      next = { text: '次 →', disabled: !canForward };
      prev = { disabled: atStart };
    } else {
      next = { text: '次 →', disabled: true };
      prev = { disabled: true };
    }
  } else {
    next = {
      text: bothReady ? '解決 →' : '次 →',
      disabled: !(bothReady || state.phase === 'reveal' ||
                  (state.phase === 'position' && !hasInput && state.cursor < state.plies.length)),
    };
    prev = { disabled: state.cursor === 0 && state.phase === 'position' && !hasInput && !state.promotionPending };
  }

  const resign = {
    visible: state.onlineMode && !state.onlineGameOver,
    disabled: state.onlineCommitted || state.onlineWaiting,
  };
  const isOver = state.onlineMode ? state.onlineGameOver : !!gameOverMsg;
  const save = { highlight: isOver };
  const startButtonsDisabled = state.watchMode;   // btn-online, btn-load
  const leaveWatchHidden = !state.watchMode;

  return { next, prev, resign, save, startButtonsDisabled, leaveWatchHidden };
}
```

- **一字一句移す**: 現行のボタン分岐をそのまま。`canForward`/`atStart` は watch/online で同一な部分だけの共有（ローカルは `!hasInput` 等が異なるので明示的に書き分ける＝現行の挙動を崩さない）。
- `resign.visible` は真偽で返し、`display` の 'inline-block'/'none' への変換は render 側で（DOM 出力を保存）。

## 2. `web/board.js` の `render()` ボタン節を薄く

```js
  const b = buttonView(state, gameOver);   // gameOver は getGameOverMsg() の結果（既存）

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
```

- **削除**: 787-841 の分岐ロジック（watch/online/local の if-else、resign/save/開始系/leave-watch の計算）。DOM 取得と代入だけ残す。
- `import { labelView, buttonView } from './view-model.js';` に更新。
- `bothReady`/`hasInput` を render の他所（overlay・View-3 まで）で使っていれば残す。ボタンのためだけに render 冒頭で計算していたなら、buttonView へ移ったぶんは不要になり得る（overlay がまだ hasInput を使うなら残す）。clippy 相当（未使用）は無いが、目視で不要な重複計算を残さない。

## 3. テスト（`web/test/view-model.test.js` に追加・golden snapshot）

`buttonView(state, gameOverMsg)` の返り値を代表 state で固定:

- **watch**: reveal/position×cursor（先頭・途中・末尾）で next.disabled・prev.disabled。
- **online 非 gameOver**: next.disabled=true・prev.disabled=true・resign.visible=true。
- **online gameOver**: ナビ解放（watch と同じ）・resign.visible=false・save.highlight=true。
- **ローカル**: bothReady（next.text='解決 →'）・hasInput（prev.disabled）・promotionPending（prev.disabled）・cursor 途中（next 有効）・gameOver（save.highlight=true）。
- **resign.disabled**: onlineCommitted/onlineWaiting で true。
- **startButtonsDisabled/leaveWatchHidden**: watchMode の真偽で反転。

## 4. 受け入れ条件

- `web/view-model.js` に `buttonView`（純粋・DOM/wasm 非依存）。`render()` がボタン導出を委譲し、ボタン DOM を vm から書くだけ。
- **ボタン DOM 出力がバイト単位で保存**（同じ state・gameOver → 同じ text/disabled/display/hidden/class）。ブラウザで各局面（観戦・オンライン対局中/終局後・ローカル各フェーズ・棋譜再生）を目視し、次/前/投了/保存/観戦離脱/対局開始系のボタンが従来と同一挙動。
- `npm test`（vitest）緑（buttonView テスト追加＋既存無傷）。
- board.js の overlay/SVG/cursor・`labelView` は無変更。engine/protocol/tui/server に差分なし。web `?v=` 前進・配布据え置き。

## 末尾要約

`render()` のボタン導出（next/prev/resign/save/観戦離脱/対局開始系の text・disabled・visible）を純粋関数 `buttonView(state, gameOverMsg)` へ抜き、view-model.js に足す。`render()` はボタン DOM を vm から書くだけに。終局メッセージは `getGameOverMsg` を render に残して結果を引数注入（純粋モジュールは wasm を呼ばず node でテストできる）。overlay/SVG/cursor は次段。golden snapshot で「同じ state → 同じボタン DOM」を守る。挙動保存・web `?v=` 前進・配布据え置き。芽 `navView`→`viewModel` の育成の二歩目。

## 不変の原則

- **表示状態は純粋・描画は薄い殻**: `buttonView(state, gameOverMsg)` がボタン状態を組み、`render()` は DOM へ流すだけ。
- **Wasm は引数注入**: `buttonView` は wasm を呼ばない（`gameOverMsg` を受ける）。node でテスト可能・ビルドレス維持。
- **挙動保存**: ボタン DOM（text・disabled・display・hidden・class）を保存。golden snapshot で守る。分岐は一字一句移す。
- **芽を育てる**: `labelView` と同じ view-model.js に `buttonView` を並べ、`viewModel` を育てる。新しい平行構造を作らない。
- **この段はボタンだけ**: overlay・SVG・cursor・`getGameOverMsg` は render に残す。触るのは view-model.js（追加）と board.js のボタン部のみ。
