# 不完全将棋 実装指示書 — board.js I/O 分解 IO-1：`handleTurnComplete` の投了判定を純粋 reduce へ

> 対象実行者: Claude Code（Sonnet 5）
> 前提: 配布 v0.12.3。`handleTurnComplete(senteUsi, goteUsi)`（board.js・522 行付近）が、投了判定（ルール 5.3/5.4 → msg/outcome/resultOverride）・合法性検証・通常 append を一つに抱えている。この段は**投了判定を純粋 `turnCompleteDecision` へ抜き**、`handleTurnComplete` を薄い I/O orchestrator にする。合法性（wasm）・通常 append・render は殻に残す。挙動保存（同じ入力 → 同じ状態遷移・I/O）。web のみ・`npm test`（vitest）で検証。
> 関連する現物（すべて実地で確認済み・HEAD `4b61937` 基準）:
> - `web/board.js` `handleTurnComplete(senteUsi, goteUsi)`（522-570）:
>   1. `state.onlineCommitted = false`。
>   2. **投了判定**（525-543）: `sResign = senteUsi==='resign'`・`gResign = goteUsi==='resign'`。両者→`'引き分け（両者投了）'`/`draw`。先手のみ→`onlineSide==='sente'?'投了しました（後手の勝ち）':'相手が投了しました（先手の勝ち）'`/`gote_wins`。後手のみ→`onlineSide==='gote'?'投了しました（先手の勝ち）':'相手が投了しました（後手の勝ち）'`/`sente_wins`。`state.resultOverride = {kind:'resign', outcome}`→`endOnlineGame(msg)`→return。
>   3. **合法性検証**（547-551）: `turnActionsAreLegal(state.sfens[state.cursor], senteUsi, goteUsi)`（**wasm**・不正な相手 reveal で resolve_ply の wasm panic を防ぐ安全弁）が false → `abortOnline('相手から非合法な着手を受信しました')`＋`endOnlineGame('中断: …')`→return。
>   4. **通常**（553-557）: `usiToText(…,'sente')`・`usiToText(…,'gote')`（**wasm**）→`branchAndAppend(…)`→`render()`。
> - `reducers.js`: 純粋 reduce 群（`resetOnlineReduce`・`hotseatConfirmReduce`）。ここに `turnCompleteDecision` を足す。
> - `turnActionsAreLegal`（wasm 依存）・`usiToText`（wasm 依存）は**殻に残す**（reduce へは渡さない）。`endOnlineGame`/`abortOnline`/`branchAndAppend`/`render` は I/O・DOM。
> - `web/test/reducers.test.js`: vitest。ここに `turnCompleteDecision` の table テストを足す。
> 関連文書: `不完全将棋_board.js_IO分解アーク_概観と段組`、`archive/board-split_総括_第零段から第三段b-3`（§3）。
> 性格: IO-1 は**「`handleTurnComplete` の投了判定（5.3/5.4 → msg/outcome/resultOverride）を純粋 `turnCompleteDecision(senteUsi, goteUsi, onlineSide)` へ抜く」**。純粋 reduce は投了 verdict だけを担い（wasm 非依存＝node テスト可能）、`handleTurnComplete` は「reduce を呼ぶ → 投了なら resultOverride＋endOnlineGame／非投了なら合法性（wasm）→ abort or 通常 append（wasm）→ render」の薄い殻に。合法性・通常・render の wasm/DOM/I/O は殻に残す。挙動保存。web のみ・`?v=` 前進・配布据え置き。

---

## 0. 目的と範囲

- **作るもの**:
  1. `web/reducers.js`: `turnCompleteDecision(senteUsi, goteUsi, onlineSide) → {kind:'resign', msg, outcome, resultOverride} | {kind:'live'}`。純粋・wasm/DOM 非依存。
  2. `web/board.js`: `handleTurnComplete` を `turnCompleteDecision` 呼び出しの薄い orchestrator に。
  3. `web/test/reducers.test.js`: `turnCompleteDecision` の table テスト。
  4. web `?v=` 前進。
- **位置づけ**: I/O 分解アークの **IO-1**（最も効く）。投了三態×視点＝6 経路がテスト可能に。
- **作らないもの（＝理由つき）**:
  - **合法性検証の reduce 化**: `turnActionsAreLegal` は wasm 依存＝殻に残す。非合法→abort の分岐（trivial）も殻。
  - **通常 append の reduce 化**: `usiToText`（wasm）・`branchAndAppend`（kifu 更新）・`render`（DOM）は殻。
  - **`enterWatchMode`/`endOnlineGame` の変更**: IO-2/IO-3。
  - **投了 verdict の Rust `game_result` との統合**: web の投了は `resultOverride` 経路（正準本文＝記録係アークの領分）。ここは web 側の msg/outcome を純粋化するだけで、統合はしない。
  - **状態遷移・I/O の結果の変更**: 挙動保存。

---

## 1. `web/reducers.js` に `turnCompleteDecision`

```js
/**
 * オンライン対局で組手が揃ったときの「投了判断」を純粋に行う。
 * 投了なら勝敗メッセージ・outcome・resultOverride を返す（ルール 5.3/5.4）。
 * 投了でなければ {kind:'live'}（合法性検証・通常 append は呼び出し側＝殻が担う）。
 * wasm 非依存（合法性・表示テキストは殻で扱う）。
 */
export function turnCompleteDecision(senteUsi, goteUsi, onlineSide) {
  const sResign = senteUsi === 'resign';
  const gResign = goteUsi  === 'resign';
  if (!sResign && !gResign) return { kind: 'live' };

  let msg, outcome;
  if (sResign && gResign) {
    msg = '引き分け（両者投了）';
    outcome = 'draw';
  } else if (sResign) {
    msg = onlineSide === 'sente' ? '投了しました（後手の勝ち）' : '相手が投了しました（先手の勝ち）';
    outcome = 'gote_wins';
  } else {
    msg = onlineSide === 'gote'  ? '投了しました（先手の勝ち）' : '相手が投了しました（後手の勝ち）';
    outcome = 'sente_wins';
  }
  return { kind: 'resign', msg, outcome, resultOverride: { kind: 'resign', outcome } };
}
```

- **一字一句移す**: 現行 `handleTurnComplete` の投了ブロック（msg/outcome の分岐）をそのまま。`resultOverride` も同形。

## 2. `web/board.js` の `handleTurnComplete` を薄い殻に

```js
function handleTurnComplete(senteUsi, goteUsi) {
  state.onlineCommitted = false;

  // 投了判断（純粋 reduce）。
  const d = turnCompleteDecision(senteUsi, goteUsi, state.onlineSide);
  if (d.kind === 'resign') {
    state.resultOverride = d.resultOverride;
    endOnlineGame(d.msg);
    return;
  }

  // 非投了: 合法性の安全弁（wasm）。不正な相手 reveal で resolve_ply が wasm パニック
  // するのを防ぐ（未検証の合法性をここで確認）。
  if (!turnActionsAreLegal(state.sfens[state.cursor], senteUsi, goteUsi)) {
    abortOnline('相手から非合法な着手を受信しました');
    endOnlineGame('中断: 相手から非合法な着手を受信しました');
    return;
  }

  // 通常: 表示テキスト（wasm）を組んで棋譜へ。
  const sText = usiToText(senteUsi, state.sfens[state.cursor], 'sente');
  const gText = usiToText(goteUsi,  state.sfens[state.cursor], 'gote');
  branchAndAppend(senteUsi, goteUsi, sText, gText);
  render();
}
```

- **削除**: 投了ブロックの msg/outcome 計算（525-543）を `turnCompleteDecision` 呼び出しに置換。`state.resultOverride = d.resultOverride` だけ殻に残す（`endOnlineGame` が I/O なので）。
- **残す**: 合法性検証（wasm・安全弁）・通常 append（wasm text・branchAndAppend・render）。これらは殻の責務。
- `import { …, turnCompleteDecision } from './reducers.js';` を足す。
- `state.onlineCommitted = false` は `update` でなく直接代入のまま（現行どおり・直後に endOnlineGame か branchAndAppend→render が描画する）。

## 3. テスト（`web/test/reducers.test.js` に追加・table）

`turnCompleteDecision(senteUsi, goteUsi, onlineSide)` を網羅:

| senteUsi | goteUsi | onlineSide | 期待 |
|---|---|---|---|
| resign | resign | （任意） | `{kind:'resign', outcome:'draw', msg:'引き分け（両者投了）', resultOverride:{kind:'resign',outcome:'draw'}}` |
| resign | 7g7f | sente | `{kind:'resign', outcome:'gote_wins', msg:'投了しました（後手の勝ち）', …}` |
| resign | 7g7f | gote | `{kind:'resign', outcome:'gote_wins', msg:'相手が投了しました（先手の勝ち）', …}` |
| 7g7f | resign | gote | `{kind:'resign', outcome:'sente_wins', msg:'投了しました（先手の勝ち）', …}` |
| 7g7f | resign | sente | `{kind:'resign', outcome:'sente_wins', msg:'相手が投了しました（後手の勝ち）', …}` |
| 7g7f | 3c3d | （任意） | `{kind:'live'}` |

- wasm 不要（純粋）。`resultOverride.outcome === outcome` の整合も確認。

## 4. 受け入れ条件

- `web/reducers.js` に `turnCompleteDecision`（純粋・wasm/DOM 非依存）。`handleTurnComplete` が投了判断を委譲し、薄い orchestrator に。
- **状態遷移・I/O が保存**: 投了三態（自陣営/相手陣営の msg）・非合法（abort＋中断）・通常（append→render）が従来と同一。ブラウザでオンライン対局し、自分の投了・相手の投了・両者投了・通常手・（可能なら）非合法受信の各挙動を目視。
- `npm test`（vitest）緑（`turnCompleteDecision` table テスト＋既存無傷）。
- 合法性検証・通常 append・render・`enterWatchMode`・`endOnlineGame` は無変更。engine/protocol/tui/server に差分なし。web `?v=` 前進・配布据え置き。

## 末尾要約

`handleTurnComplete` の投了判定（ルール 5.3/5.4 → msg/outcome/resultOverride）を純粋 `turnCompleteDecision(senteUsi, goteUsi, onlineSide)` へ抜き、`reducers.js` に足す。純粋 reduce は投了 verdict だけを担い（wasm 非依存＝node テスト可能）、`handleTurnComplete` は「reduce を呼ぶ → 投了なら resultOverride＋endOnlineGame／非投了なら合法性（wasm）→ abort or 通常 append（wasm）→ render」の薄い殻になる。合法性・通常・render の wasm/DOM/I/O は殻に残す（本質的 I/O を捻じらない）。table テストで投了三態×視点＝6 経路を守る。挙動保存・web `?v=` 前進・配布据え置き。I/O 分解アークの最初にして最も効く一段。

## 不変の原則

- **純粋判断は reduce・I/O は薄い殻**: 投了 verdict は `turnCompleteDecision`（純粋）。`handleTurnComplete` は reduce を呼んで update＋I/O するだけ。
- **Wasm・I/O は殻**: `turnActionsAreLegal`/`usiToText`/`endOnlineGame`/`abortOnline`/`branchAndAppend`/`render` は殻。純粋 reduce には持ち込まない。
- **挙動保存**: 投了・非合法・通常の状態遷移と I/O を保存。table テストで守る。判断は一字一句移す。
- **本質的 I/O は捻じらない**: 合法性（wasm 安全弁）・append・render は純粋化しない。純粋なのに埋まっていた投了 verdict だけを抜く。
- **この段は投了だけ**: `enterWatchMode`（IO-2）・`endOnlineGame`（IO-3）は次段。触るのは reducers.js（追加）と board.js の handleTurnComplete のみ。
