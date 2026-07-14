# 不完全将棋 board.js I/O 分解アーク（種類2）— 概観と段組

> 対象読者: このアークを実装する Claude Code（Sonnet 5）、各段の指示書、次の Opus セッション。
> 位置づけ: **設計方針の錨**。board.js の I/O 絡みの三関数（`handleTurnComplete`・`enterWatchMode`・`endOnlineGame`）に埋め込まれた**純粋な判断ロジックを純粋 reduce へ抜き**、I/O 関数を薄い orchestrator にする。動機は可読性、実利は**テストカバレッジ**——埋め込みで届かなかった分岐（投了三態・非合法・再戦・記録係食い違い等）を table テストで守る。
> 前提の現在地: 配布 v0.12.3。board.js 1130 行。分割アーク完了時の残り本丸「種類2」（総括 §3・バックログ §D）。view 純粋化アークは着地済み。web は既に reduce パターンを持つ（`reducers.js`: `resetOnlineReduce`・`hotseatConfirmReduce`／`confirmMove` は `update(hotseatConfirmReduce(...))`）。
> 関連文書: `archive/board-split_総括_第零段から第三段b-3`（§3・確立した設計パターン：状態器→update→reduce）、`archive/board.js_view純粋化アーク_概観と段組`。

---

## 0. なぜこのアークか

三関数は「非同期 I/O（WS 送信・connectSpectate コールバック・setTimeout・記録係待ち）」と「状態遷移」が縒れている。埋め込まれた判断（投了判定 5.3/5.4、再戦時のリセット、記録係イベントの反映）は**純粋なのにテストできない**——I/O に絡んでいるから。これを純粋 reduce に抜けば、`reducers.js` の既存パターン（`hotseatConfirmReduce` 等）に揃い、table テストで分岐を尽くせる。可読性が上がり、テストカバレッジが実利として付いてくる。

## 1. パターン（種類1 の reduce を I/O 関数へ広げる）

- **埋め込みの純粋判断を reduce へ**: 「この入力 → 状態 patch（または分類）」を `reducers.js` の純粋関数に。I/O 関数は reduce を呼んで `update(patch)` するだけの薄い殻に。
- **Wasm・I/O は注入 or 殻に残す**: `turnActionsAreLegal`（合法性・wasm）・`usiToText`（表示・wasm）・`currentResult`（wasm）・送信/切断/setTimeout は殻（I/O 関数）で。純粋 reduce にはその**結果**を渡す（引数注入）。純粋 reduce は wasm を呼ばない＝node でテスト可能。
- **北極星は tui の online.rs**: 通信核の一本化で `ClientSession`（純粋遷移）＋`Transport`（I/O）＋`resolve_completed_turn`（薄い orchestrator）に書き換わった。web の I/O 関数もこの「純粋遷移／I/O」の分離に倣う。

## 2. 目標構造（例：handleTurnComplete）

```
handleTurnComplete(sUsi, gUsi):            ← 薄い I/O orchestrator
  update({ onlineCommitted: false });
  const d = turnCompleteDecision(sUsi, gUsi, state.onlineSide);   ← 純粋（投了 verdict）
  if (d.kind === 'resign') { update({resultOverride: d.resultOverride}); endOnlineGame(d.msg); return; }
  if (!turnActionsAreLegal(sfen, sUsi, gUsi)) { abortOnline(...); endOnlineGame(...); return; }  ← wasm は殻
  branchAndAppend(sUsi, gUsi, usiToText(...), usiToText(...)); render();   ← wasm/DOM は殻
```

純粋 reduce（`reducers.js`・wasm/DOM 非依存・テスト可能）と、薄い I/O 関数（wasm・送信・DOM を持つ）に割れる。

## 3. 段組（依存 IO-1 → IO-2 → IO-3・各段独立に検証）

- **IO-1 — `handleTurnComplete`**: 投了判定（5.3/5.4 → msg/outcome/resultOverride）を純粋 `turnCompleteDecision(senteUsi, goteUsi, onlineSide) → {kind:'resign',…} | {kind:'live'}` へ抜く。合法性（wasm）・通常 append・render は殻に残す。**最も効く**（投了三態×視点＝6 経路がテスト可能に）。
- **IO-2 — `enterWatchMode`**: connectSpectate のコールバックが組む state patch を純粋 reduce へ。`watchInitReduce(state, {version, initial_sfen, turns, result})`（turns→plies・loadedMeta）・`watchMetaReduce(state, {version, initial_sfen})`（再戦リセット）・小さな reduce（status/result/record 系）。loadPlies（wasm 再生）は殻に残す。再戦・記録係食い違い・archived の分岐がテスト可能に。
- **IO-3 — `endOnlineGame`**: 終局 patch を純粋 `endGameReduce(msg) → patch` へ。`isRecording` 分岐と effect 列（spectate/record 送信・disconnect・記録係待ち setTimeout）は本質的 I/O なので殻に残し、薄く保つ。純粋部は小さいが、終局 patch がテスト可能に。

各段の詳細（現物の行・関数）は、前段が着地して**リポジトリを取り直してから**書く（想像で書かない）。

## 4. テスト（table テスト・実利の本体）

`web/test/reducers.test.js` を拡張。純粋 reduce を wasm 非依存で網羅:
- `turnCompleteDecision`: 先手投了/後手投了/両者投了 × onlineSide（sente/gote）→ msg/outcome/resultOverride、非投了 → `{kind:'live'}`。
- `watchInitReduce`/`watchMetaReduce`: turns→plies・loadedMeta、再戦リセット、result 有無。
- 小 reduce（status/result/record）: 各イベント → patch。
- `endGameReduce`: msg → 終局 patch。
- wasm は結果注入なので node（vitest）で走る（ビルドレス維持）。

## 5. 版

- 各段は挙動保存リファクタ（同じ入力 → 同じ状態遷移・I/O）。**配布据え置き・web `?v=` 前進**。利用者に見える変化なし。

## 不変の原則

- **純粋判断は reduce・I/O は薄い殻**: 埋め込みの判断を `reducers.js` の純粋関数へ。I/O 関数は reduce を呼んで update＋副作用を行うだけ。
- **Wasm・I/O は注入 or 殻**: `turnActionsAreLegal`/`usiToText`/`currentResult`/送信/切断/setTimeout は殻。純粋 reduce にはその結果を渡す。node テスト可能・ビルドレス維持。
- **挙動保存**: 状態遷移と I/O の結果を保存。table テストで分岐を守る。判断ロジックは一字一句移す。
- **本質的 I/O は捻じらない**（過ぎたるは及ばざる）: `endOnlineGame` の effect 列や記録係待ちを人工的に「純粋化」しない。純粋なのに埋まっている判断だけを抜く。
- **北極星は tui online.rs**: 純粋遷移／I/O の分離に倣う（通信核一本化後の姿）。
- **細かく刻む**: IO-1→2→3、各段を table テストで検証してから次へ。
