# 不完全将棋 実装指示書 — board.js I/O 分解 IO-3（締め）：`endOnlineGame` の終局 patch を純粋 reduce へ

> 対象実行者: Claude Code（Sonnet 5）
> 前提: IO-2 着地（HEAD `2b5e14a`。`reducers.js` に `turnCompleteDecision`・`metaToLoadedMeta`・`archivedLinkFor`）。`endOnlineGame(msg)`（board.js）は**ほぼ全体が本質的 I/O**（spectate 送信・記録係証言・setTimeout の記録係待ち・disconnect・wasm の `currentResult`/`buildArchiveText`）。純粋なのは末尾の終局 patch のみ。この段は**終局 patch を純粋 `endGameReduce(msg)` へ抜き**（`resetOnlineReduce` の終局版）、effect 列は本質的 I/O として殻に残す。これで三関数（handleTurnComplete・enterWatchMode・endOnlineGame）すべてが「純粋遷移＋薄い I/O」に揃い、**I/O 分解アークが綴じる**。挙動保存。web のみ・`npm test`（vitest）で検証。
> 関連する現物（すべて実地で確認済み・HEAD `2b5e14a` 基準）:
> - `web/board.js` `endOnlineGame(msg)`:
>   - `const result = currentResult();`（**wasm**）→ `sendSpectateResult(result.kind, result.outcome);`（**I/O**・ws を閉じる前に）。
>   - `if (isRecording())`（純粋 state 読み）→ `sendRecordTestimony(result.kind, result.outcome, buildArchiveText());`（**I/O**・buildArchiveText は wasm）→ `state._pendingRecordDisconnect = true; setTimeout(() => {…disconnectOnline()…}, 5000);`（**I/O**・記録係の archived/disagreement を待つ保険タイムアウト）。else → `disconnectOnline();`（**I/O**）。
>   - 末尾: `update({ onlineGameOver: true, onlineEndMsg: msg, onlineCommitted: false, onlineWaiting: false });`（**純粋 patch**・この段の対象）。
> - `web/reducers.js` `resetOnlineReduce()`（オンライン状態リセット）: `endGameReduce` はその終局版として並べる。
> - I/O（殻に残す）: `currentResult`（wasm）・`sendSpectateResult`・`sendRecordTestimony`・`buildArchiveText`（wasm）・`isRecording`・`disconnectOnline`・`setTimeout`。
> - `web/test/reducers.test.js`: vitest。
> 関連文書: `不完全将棋_board.js_IO分解アーク_概観と段組`、IO-1・IO-2 指示書。
> 性格: IO-3 は**「`endOnlineGame` の終局 patch（`{onlineGameOver, onlineEndMsg, onlineCommitted:false, onlineWaiting:false}`）を純粋 `endGameReduce(msg)` へ抜く」**。effect 列（spectate 送信・記録係証言・setTimeout 待ち・disconnect）は本質的 I/O なので**そのまま殻に残す**（人工的に純粋化しない＝アークの原則）。小さな締めだが、これで三関数すべてが純粋遷移＋薄い I/O に揃う。挙動保存。web のみ・`?v=` 前進・配布据え置き。**I/O 分解アークの締め**。

---

## 0. 目的と範囲

- **作るもの**:
  1. `web/reducers.js`: `endGameReduce(msg) → { onlineGameOver: true, onlineEndMsg: msg, onlineCommitted: false, onlineWaiting: false }`。純粋。
  2. `web/board.js`: `endOnlineGame` 末尾の `update({...})` を `update(endGameReduce(msg))` に。effect 列は無変更。
  3. `web/test/reducers.test.js`: `endGameReduce` のテスト。
  4. web `?v=` 前進。
- **位置づけ**: I/O 分解アークの **IO-3（締め）**。終局の状態遷移が named・tested reduce に。三関数の分解が揃う。
- **作らないもの（＝理由つき）**:
  - **effect 列の純粋化**: `sendSpectateResult`・`sendRecordTestimony`・記録係待ちの `setTimeout`・`disconnectOnline` は本質的 I/O。捻じらず殻に残す（アーク概観 §1・過ぎたるは及ばざる）。記録係の 5 秒保険タイムアウトのコメント（rationale）も保つ。
  - **`currentResult`/`buildArchiveText` の移設**: wasm。殻に残す。
  - **`isRecording` 分岐の reduce 化**: 分岐は effect（証言＋待ち vs 即切断）を分ける I/O 判断。殻に残す。
  - **`handleTurnComplete`（IO-1）/`enterWatchMode`（IO-2）の変更**: 済み。

---

## 1. `web/reducers.js` に `endGameReduce`

```js
/**
 * オンライン対局の終局時の状態 patch。純粋。（resetOnlineReduce の終局版。）
 * effect（spectate/record 送信・disconnect・記録係待ち）は呼び出し側＝殻が行う。
 */
export function endGameReduce(msg) {
  return {
    onlineGameOver: true,
    onlineEndMsg: msg,
    onlineCommitted: false,
    onlineWaiting: false,
  };
}
```

- 現行 `update({...})` の中身を一字一句移す。

## 2. `web/board.js` の `endOnlineGame` 末尾を差し替え

```js
function endOnlineGame(msg) {
  // …（effect 列は無変更: currentResult / sendSpectateResult / isRecording 分岐 /
  //    sendRecordTestimony / setTimeout 記録係待ち / disconnectOnline。
  //    コメント〔ws を閉じる前に送る・記録係二段目 §10・5 秒保険タイムアウト〕も保つ）…

  update(endGameReduce(msg));
}
```

- **変更は末尾一行のみ**: `update({ onlineGameOver: true, … })` → `update(endGameReduce(msg))`。
- `import { …, endGameReduce } from './reducers.js';` を足す。
- effect 列（先頭〜setTimeout/disconnect）は**完全に無変更**。

## 3. テスト（`web/test/reducers.test.js` に追加）

- `endGameReduce('引き分け（両者投了）')` → `{ onlineGameOver: true, onlineEndMsg: '引き分け（両者投了）', onlineCommitted: false, onlineWaiting: false }`。
- 任意の msg で `onlineEndMsg === msg`、他 3 フィールドが固定であること。

## 4. 受け入れ条件

- `web/reducers.js` に `endGameReduce`（純粋・wasm/DOM 非依存）。`endOnlineGame` 末尾が `update(endGameReduce(msg))`。
- **状態遷移・I/O が保存**: 終局時の spectate 送信・記録係証言＋待ち・即切断・終局 patch が従来と同一。ブラウザでオンライン対局を終局させ（通常終局・投了・記録係あり/なし）、終局表示・切断・記録係待ちが従来どおり。
- `npm test`（vitest）緑（`endGameReduce` テスト＋既存無傷）。
- effect 列・`handleTurnComplete`・`enterWatchMode` は無変更。engine/protocol/tui/server に差分なし。web `?v=` 前進・配布据え置き。

## 5. アークの締め

IO-3 着地で **I/O 分解アークが綴じる**。三関数（`handleTurnComplete`・`enterWatchMode`・`endOnlineGame`）はいずれも「純粋遷移（`reducers.js` の reduce）＋薄い I/O」に揃い、埋め込みで届かなかった判断（投了三態×視点・meta マッピング・archivedLink 分岐・終局 patch）が table テストで守られる。総括を綴じ、バックログ §D から「種類2＝I/O 分解」を落とす。**これで board.js の残り本丸（view 純粋化＋I/O 分解）が尽き**、当初の狙い——ルール変更を安全でテスト可能な地盤の上で進める——が整った。次はルール変更のアイデアへ、実感が向くときに。

## 末尾要約

`endOnlineGame` の終局 patch（`{onlineGameOver, onlineEndMsg, onlineCommitted:false, onlineWaiting:false}`）を純粋 `endGameReduce(msg)` へ抜き、`reducers.js`（`resetOnlineReduce` の終局版）に足す。変更は末尾一行のみで、effect 列（spectate 送信・記録係証言・5 秒保険タイムアウト・disconnect）は本質的 I/O として無変更で殻に残す。小さな締めだが、これで三関数すべてが純粋遷移＋薄い I/O に揃い、I/O 分解アークが綴じる。board.js の残り本丸が尽き、ルール変更の地盤が整う。挙動保存・web `?v=` 前進・配布据え置き。

## 不変の原則

- **純粋遷移は reduce・I/O は薄い殻**: 終局 patch は `endGameReduce`。effect 列は殻。
- **本質的 I/O は捻じらない**: spectate/record 送信・記録係待ち setTimeout・disconnect は純粋化しない。純粋なのに埋まっていた終局 patch だけを抜く。
- **挙動保存**: 終局の状態遷移と I/O（送信・待ち・切断）を保存。記録係待ちの rationale コメントも保つ。
- **この段で締める**: 三関数の分解が揃う。総括を綴じ、バックログ §D から落とす。触るのは reducers.js（追加）と board.js の endOnlineGame 末尾のみ。
