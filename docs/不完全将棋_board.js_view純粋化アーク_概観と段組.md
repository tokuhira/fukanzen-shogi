# 不完全将棋 board.js view 純粋化アーク — 概観と段組

> 対象読者: このアークを実装する Claude Code（Sonnet 5）、各段の指示書、次の Opus セッション。
> 位置づけ: **設計方針の錨**。`render()` の「state → 表示値」の導出を純粋関数へ抜き、`render()` を薄い DOM 書き込みに残す。パターン・段組・テスト・北極星をここに綴じる。
> 前提の現在地: 配布 v0.12.3。board.js 分割アークの残り本丸の一つ（総括 §3・バックログ §D の「view 純粋化」）。もう一つの本丸「種類2＝I/O 分解」は**別アーク**（頑健性向上を畳み込む・脅威が切迫したら順番反転）。
> 関連文書: `archive/board-split_総括_第零段から第三段b-3`（§3 次のアーク・確立した設計パターン）、`design/不完全将棋_版図_世界観と設計方針`（核と交換可能な殻）。

---

## 0. なぜこのアークか

ルール変更を見据えた board.js の整理。ルール変更が触るのは主に**表示状態と終局メッセージ**——それは `render()` の `phaseText`/gameOver 分岐に集中している。ここを「同じ `state` → 同じ表示値」の純粋関数にすれば、ルール駆動の表示追加が安全でテストできる。頑健性（悪意入力）とは直交（脅威の入り口は摂取点＝種類2 の三関数であって render ではない）。

## 1. パターン（分割アークの原則を view に適用）

- **純粋な芯を抜き、殻に薄いラッパ**: `render()` から「state → 表示値」の計算を純粋モジュールへ。`render()` には DOM 書き込みだけ残す。
- **Wasm は引数注入**: 純粋モジュールは wasm を呼ばない。盤面から導く終局メッセージ（`getGameOverMsg`＝wasm＋メモ化）は**結果を引数で受ける**（`viewModel(state, gameOverMsg)`）。これで node（vitest）でテストできる。
- **北極星は `navView()`**: 既にある `navView()`（`{phase, cursor, pliesLen, onlineMode, onlineGameOver}` を返す純粋スナップショット）が芽。このアークはそれを**完全な `viewModel` へ育てる**。

## 2. 目標構造

```
render():
  const pos          = parseSfen(state.sfens[state.cursor]);      // 盤面（純粋）
  const gameOverMsg  = getGameOverMsg();                          // wasm＋メモ化（殻）
  const vm           = viewModel(state, gameOverMsg);             // ← 純粋な表示値の束
  // …vm から薄い DOM 書き込みだけ…
```

`viewModel(state, gameOverMsg)` が返す表示値（純粋・DOM 非依存・wasm 非依存）:
- **ラベル**（View-1）: `phaseText`・`moveText`・`eventText`・`archiveInfo{text,mismatch}`・`step`・`total`
- **ボタン**（View-2）: `next{text,disabled}`・`prev{disabled}`・`resign{visible,disabled}`・`save{highlight}`・`leaveWatch{hidden}`・`onlineLoad{disabled}`
- **overlay/cursor**（View-3）: `overlay`（`revealOverlay`/`inputOverlay` の結果）・`cursorInteractive`（SVG カーソルの真偽）

`_watchPhaseText`/`_onlinePhaseText`/`archiveInfoText` は state を引数に取る純粋関数として viewModel モジュールへ移す（現状は module global `state` を読むが、いずれも DOM 非依存）。

## 3. 段組（依存 View-1 → View-2 → View-3・各段独立に検証可）

- **View-1 — ラベルの純粋化**: `web/view-model.js` を新設し `labelView(state, gameOverMsg) → {phaseText, moveText, eventText, archiveInfo, step, total}` を置く（`_watchPhaseText`/`_onlinePhaseText`/`archiveInfoText` を state 引数化して同梱）。`render()` はラベル系 DOM（phase-label/move-display/event-label/archive-info/step-label）を vm から書くだけに。golden snapshot テストを据える。**芽 `navView` の最初の拡張**。
- **View-2 — ボタンの純粋化**: `buttonView(state, gameOverMsg) → {…}` を追加。`render()` のボタン分岐（next/prev/resign/save/leaveWatch/online-load）を vm から書くだけに。テスト追加。
- **View-3 — overlay/cursor の純粋化**: `overlay` と `cursorInteractive` を viewModel へ。`render()` は `renderSvg(pos, vm.overlay)`＋カーソル設定だけに。ここで `render()` は「vm を作って DOM に流す」薄い殻になり、`navView` は完全な `viewModel` へ育ち切る（`navView` の呼び出し元があれば `viewModel` の部分ビューへ寄せる）。

## 4. テスト（golden snapshot）

`web/test/` に viewModel のテスト。代表的な `state`（＋`gameOverMsg`）を束で用意し、返る表示値を固定する: reveal・観戦（connecting/error/closed/player_disconnected/concluded）・オンライン（waiting/committed/gameOver）・bothReady・pending・gameOver・初期局面・cursor 途中。ルール変更で終局種別や phaseText が増えたら、ここに局面を足して守る（総括 §3 の「golden snapshot への局面追加もここで」）。wasm は `gameOverMsg` 引数注入なので node で走る（ビルドレス維持）。

## 5. 版

- 各段は挙動保存リファクタ（同じ state → 同じ描画）。**配布据え置き・web `?v=` 前進**（board.js 変更のキャッシュ更新）。
- 利用者に見える変化なし。DOM 出力はバイト単位で保存する（View-1〜3 の受け入れ条件の中心）。

## 不変の原則

- **描画は薄い殻・表示値は純粋**: `viewModel(state, gameOverMsg)` が表示値を組み、`render()` は DOM へ流すだけ。
- **Wasm は引数注入**: 純粋モジュールは wasm を呼ばない（`gameOverMsg` を受ける）。node でテスト可能・ビルドレス維持。
- **挙動保存**: DOM 出力（textContent・class・disabled・innerHTML・style）を保存する。golden snapshot で「同じ state → 同じ描画」を守る。
- **芽を育てる**: `navView` を完全な `viewModel` へ。新しい平行構造を作らず既存の芽を伸ばす。
- **頑健性とは直交**: 悪意入力の摂取点は種類2（別アーク）。view 純粋化は表示の関心事に閉じる。
- **細かく刻む**: ラベル→ボタン→overlay の順に、各段を独立に検証（golden snapshot）してから次へ。
