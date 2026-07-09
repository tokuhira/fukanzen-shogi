# 不完全将棋 実装指示書 — 第三段b-1：`kifu` を器へ吸収し、状態更新を一本の経路に通す（書き込み集約の下地）

> 対象実行者: Claude Code（Sonnet 5 を推奨。board.js の広範囲に触れる。挙動不変の担保が要）。
> 前提: 配布 v0.11.2 / web `?v=`0.11.8（board.js 分割 第三段a まで着地。30 状態変数が単一の `const state` に集約済み。web テスト 42 件・golden snapshot・純粋モジュール 7 本が据わっている）。
> 関連する現物（すべて実地で確認済み）:
> - `const kifu = { plies: [] }`（77）が `state` の隣に据え置かれている（第三段a の申し送り）。`kifu.plies` の参照は約 20 箇所（大半は読み取り `kifu.plies.length`／`kifu.plies[state.cursor]`、書き込みは `setRecord` の 1 箇所のみ）。
> - `state.<名> = 値` の直接代入は board.js 内に **133 箇所**。`render()` 呼び出しは **43 箇所**（状態更新のたびに手で呼んでいる）。
> - **相似形の北極星（既存の tui 盤）**: `tui/src/app.rs` の `struct App` は `kifu`・`phase`・`cursor`・`selection` を**一つの器に集約**し（web の目標形と一致）、`on_board_press`/`confirm_move`/`resolve_turn`/`undo`/`clear_selection`/`new_game` 等の**メソッドで遷移**する。描画は `main.rs` の `loop { terminal.draw(|f| ui::draw(f, app)) }` で状態更新と分離して回る。web の各所 `render()` 呼び出しは、この「更新と描画の分離」に対応させられる。I/O（ネット対戦）は `tui/src/online.rs` に分離され「ゲームロジックは App を再利用する」——これが第三段b-2 で種類1の遷移を純粋化する際の参照点。
> - 状態は board.js 内に閉じ export されていない（online.js は callbacks 連携）。本段の変更は board.js 一枚で完結。
> 関連文書: `不完全将棋_実装指示書_状態を単一のstate器へ集約_board分割第三段a`、`不完全将棋_実装指示書_棋譜コアの遷移を純粋化_board分割第二段a`（`setRecord`/`currentRecord`）、`不完全将棋_版図_世界観と設計方針`（核と交換可能な殻）、`不完全将棋_バックログ_伏線と未決`。
> 性格: 第三段b-1 は**「`kifu` を `state` へ吸収して器を完全に一つにし、状態更新を一本の経路（`update`）に通す下地を作る」**。読み書き二段作戦の**書き込み集約の前半**。ここでは**意味のある遷移の純粋化（reducer 化）はしない**——それは b-2。b-1 は「全書き込みが一箇所を通り、描画が更新に従属する」構造を機械的に作るだけ。tui の `App` を北極星に、web を「器＋更新経路＋従属する描画」へ寄せる。Rust に触れず Wasm 再ビルドなし。製品挙動は完全に不変。行番号は v0.11.8 基準。

---

## 0. 目的と範囲

- **作るもの**:
  1. `kifu` を `state` へ吸収（`state.plies`）。器を完全に一つにする。`setRecord`/`currentRecord` を整合。
  2. 状態更新の一本の経路 `update(patch)` を導入。`state.x = v` の直接代入を `update({ x: v })` へ寄せ、**`update` の中で `render()` を一度呼ぶ**ことで、散在する 43 の `render()` 呼び出しを構造的に畳む下地を作る。
  3. 挙動不変の担保: 既存 42 テスト（特に golden snapshot）が緑のまま。
- **位置づけ**: board.js 分割の**第三段b-1**。書き込み集約の下地。全書き込みが `update` を通り、描画がそれに従属する構造にする（tui の「App 更新→draw」に相似）。意味のある遷移（アクション）の純粋化・reducer 化は b-2。
- **作らないもの（＝理由つき）**:
  - **純粋 reducer・アクションの定義**: `goPrev` の局面遷移・`_resetOnlineState` 等を純粋関数へ抜くのは **b-2**。b-1 で意味の整理に踏み込むと範囲が二重に膨らむ。b-1 はあくまで「経路を通す」機械的作業。
  - **非同期 I/O 絡みの遷移の分解**（`enterWatchMode` のコールバック・`endOnlineGame` の setTimeout）: これらの**状態更新**は `update` を通すが、**I/O（`connectSpectate`/`disconnectOnline`/`sendRecordTestimony` 等）はそのまま**。I/O の分離は tui の online.rs 相当で、b-2 以降の検討。
  - **描画の完全な一元化**（全 `render()` 撤去）: b-1 では `update` が render を呼ぶ形を作るが、`update` を経由しない特殊な描画（読み込み中表示など）や、`update` 一回にまとめきれない箇所は**残してよい**。過ぎたるは及ばざる——まず経路を通し、撤去は段階的に。

---

## 1. `kifu` の `state` への吸収

`const kifu = { plies: [] }` を削除し、`state` に `plies: []` を加える（`state.plies`）。`kifu.plies` の全参照（約 20 箇所）を `state.plies` へ:

- 読み取り: `kifu.plies.length` → `state.plies.length`、`kifu.plies[state.cursor]` → `state.plies[state.cursor]`、`kifu.plies.slice(...)` → `state.plies.slice(...)`、`kifu.plies.map(...)` → `state.plies.map(...)`。
- 書き込み: `setRecord` の `kifu.plies = record.plies` → `state.plies = record.plies`（b-1 導入の `update` を使うなら `update({ sfens: record.sfens, events: record.events, plies: record.plies })`、§2 参照）。
- `currentRecord` → `return { sfens: state.sfens, events: state.events, plies: state.plies }`。

これで `state` が唯一の器になる（tui の `App { kifu, phase, cursor, … }` に相似——ただし web は plies をフラットに `state.plies` として持つ）。

## 2. 更新経路 `update(patch)` の導入

`state` 定義の直後に、状態を更新して再描画する一本の経路を置く:

```js
// 状態更新の唯一の経路。patch を state へ浅くマージし、一度だけ再描画する。
// （tui の「App を更新 → terminal.draw」の分離に相似。描画は更新に従属する。）
// 注意: b-1 では reducer 化はしない——単純な浅いマージ。意味のある遷移の整理は b-2。
function update(patch) {
  Object.assign(state, patch);
  render();
}
```

- **`render()` を呼ばない純粋な代入が要る箇所**（描画を伴わない中間更新、ループ内の逐次更新など）は、`update` ではなく `Object.assign(state, patch)` 直接、または `state.x = v` のままでよい（b-1 は全撤去を目的にしない）。判断基準: 「この代入の直後に render したいか」。したいなら `update`、したくない/後でまとめて render するなら直接代入。

### 変換の指針（機械的・意味不変）

現在「`state.x = v; …; render();`」となっている塊を「`update({ x: v, … });`」へ畳む。例:

```js
// before（confirmMove のホットシート分岐末尾）:
state.selectedFrom = null; state.legalTargets = null;
state.promotionPending = null; hidePromotionUI();
// after: hidePromotionUI() は DOM 副作用なので update の外。状態は update へ。
hidePromotionUI();
update({ selectedFrom: null, legalTargets: null, promotionPending: null });
```

```js
// before（goPrev の局面遷移）:
} else if (state.phase === 'position' && state.cursor > 0) {
  state.cursor--;
  state.phase = 'reveal';
}
render();
// after:
} else if (state.phase === 'position' && state.cursor > 0) {
  update({ cursor: state.cursor - 1, phase: 'reveal' });
} else { render(); }   // 他分岐で状態不変でも描画は要る場合は個別に
```

- **重要な原則**: `update` への畳み込みは**意味を変えない範囲で**。1 回の論理的な状態遷移につき `update` 1 回が理想だが、無理に 1 回へ詰めない。複数 `update` に分かれても、各々が `render` を呼ぶだけで挙動は不変（描画が余分に走るだけ、結果は同じ）。**まず全書き込みを `update` か明示的直接代入のどちらかに分類し、散らばった `render()` を減らす**のが b-1 のゴール。
- **`render()` の重複呼び出しに注意**: `update` が render を含むので、`update({...}); render();` のように二重にしない。`update` へ畳んだら末尾の `render()` は消す。

## 3. 非同期 I/O 絡みの扱い（状態更新は経路へ・I/O はそのまま）

`enterWatchMode`・`endOnlineGame`・`handleTurnComplete` 等、I/O と絡む関数でも、**状態更新は `update` を通す**が I/O 呼び出しは変えない:

```js
// enterWatchMode のコールバック内（例）:
onArchived: (id) => {
  update({ recordStatusText: '記録されました', archivedLink: { id, url: archiveUrl(id) } });
},
// connectSpectate(...) 自体は I/O なのでそのまま。
```

```js
// endOnlineGame（I/O は残す）:
function endOnlineGame(msg) {
  update({ onlineGameOver: true, onlineEndMsg: msg, onlineCommitted: false, onlineWaiting: false });
  const result = currentResult();
  sendSpectateResult(result.kind, result.outcome);   // I/O: そのまま
  if (isRecording()) {
    sendRecordTestimony(result.kind, result.outcome, buildArchiveText());  // I/O
    state._pendingRecordDisconnect = true;            // 単発フラグ: 直接でも update でも可
    setTimeout(() => { … }, 5000);                    // I/O タイマ: そのまま
  } else {
    disconnectOnline();                               // I/O: そのまま
  }
  // 末尾の render() は update が担うので不要（update を最初に呼んでいれば）
}
```

- `_pendingRecordDisconnect` のような「render を伴わない内部フラグ」は `state._pendingRecordDisconnect = true` の直接代入でよい（描画不要）。無理に `update` にしない。

## 4. 受け入れ（挙動不変の担保が中心）

- `cd web && npm test` が緑（既存 42 件がすべて、**snapshot 差分ゼロ**。新規テストは無し——b-1 は経路の付け替えで新しい振る舞いを足さない）。
- `node --check web/board.js` が通る。
- **機械確認**:
  - `grep -n "kifu" board.js` → 0 件（`state.plies` へ完全吸収。コメント内の歴史的言及は可）。
  - `grep -nE "update\(\{[^}]*\}\);\s*render\(\)" board.js` → 0 件（update 後の二重 render なし）。
  - `render()` 呼び出し総数が 43 から**減っている**こと（`update` へ畳んだぶん。ゼロにはならない）。
- ブラウザで**全機能の手触りが従来と同一**（第三段a と同じ全項目）: 新規対局・棋譜読込・着手選択と確定・同時開示・分岐・アーカイブ保存/読込・オンライン（commit/reveal/投了/切断）・観戦（ライブ追記・再戦・記録係通知・リンク）・記録係・終局判定・棋譜ナビ（← →）・盤/持ち駒クリック・成り選択 UI。**特に**: 観戦のコールバック経由の状態更新（`onTurn`/`onArchived`/`onMeta`）が従来通り描画に反映されること、`endOnlineGame` の投了・記録係綴じ待ち・切断タイマが従来通り動くこと。

## 5. 版の刻み

- **製品挙動は完全に不変・Rust 非関与・Wasm 再ビルドなし**。board.js の広範囲に触れる。整備・挙動不変リファクタとして配布版据え置き **v0.11.2**、web の `?v=`（`web/package.json`・`web/index.html`）を **0.11.9** へ前進。**RULE 0.6・PROTOCOL 4・アーカイブ書式 1 不変**。

## 6. 申し送り（第三段b-2＝書き込み集約の本体＝reducer 化へ）

- 器が一つ（`state` に `plies` まで吸収）になり、全書き込みが `update`（か明示的直接代入）を通る下地ができた。次は **b-2＝意味のある遷移の純粋化**:
  - **種類1（純粋な状態遷移）を reducer/純粋関数へ抜きテストで固める**: `goPrev` の局面移動（reveal↔position・cursor 増減）、`_resetOnlineState`（11 変数のリセット）、`confirmMove` のホットシート分岐（pending セット・inputStep 進行）。tui の `App` メソッド（`undo`/`on_escape`/`new_game`/`confirm_move`）が粒度の北極星。`(state, action) → 次state` の純粋関数にして vitest で固定（Wasm 不要な遷移が多い）。
  - **種類2（I/O 絡み）は tui の online.rs に倣い分離を検討**: `handleTurnComplete`・`enterWatchMode`・`endOnlineGame` は「純粋な状態遷移」と「I/O」に割る。純粋部分は reducer へ、I/O は殻に残す（「ゲームロジックは核を再利用する」）。
- **view の純粋化**（render() 本体の phaseText/ボタン分岐を `state` スナップショット→表示値の純粋関数へ）は、b-2 で遷移が固まった後。集約前後で「同じ `state` → 同じ描画」を守れる。
- golden snapshot への局面追加（第一段a の申し送り）は view 段で。

---

## 7. 不変の原則（本実装が守るもの）

1. **器は一つ**: `kifu` を `state.plies` へ吸収。状態は `state` のみ（tui の `App` に相似）。
2. **書き込みは一本の経路へ**: 状態更新は `update(patch)`（浅いマージ＋再描画）を通す。描画は更新に従属する（tui の「App 更新→draw」）。b-1 は reducer 化しない——単純マージのみ。
3. **意味を変えない**: `update` への畳み込みは機械的。1 遷移 1 update が理想だが無理に詰めない。二重 render を作らない。
4. **I/O は殻に残す**: 非同期 I/O（connect/disconnect/send/timer）は変えず、その中の**状態更新だけ** `update` を通す。I/O の分離は b-2 以降。
5. **挙動不変を snapshot と全機能手触りで担保**: 新規テストは足さず、既存 42 件（特に golden snapshot）の緑と機械確認で守る。Rust に触れず Wasm 再ビルドなし。配布版据え置き、web `?v=` のみ前進。

---

*第三段b-1——`kifu` を器へ吸収し、状態更新を一本の経路に通す。読み書き二段作戦の書き込み集約の前半。`const kifu` を `state.plies` へ畳んで器を完全に一つにし（tui の `struct App` が kifu を内に持つのと同じ形）、散在する 133 の代入と 43 の render を、状態更新の唯一の経路 `update(patch)`（浅いマージ＋再描画）へ寄せる。描画が更新に従属する構造——tui の「App を更新して terminal.draw」に相似——を機械的に作る。意味のある遷移の純粋化（reducer 化）はまだしない、それは b-2。非同期 I/O（観戦コールバック・記録係綴じ待ち・切断タイマ）は殻に残し、その中の状態更新だけ経路を通す。北極星は tui の App——web を「器＋更新経路＋従属する描画」へ寄せ、三つ目の盤（スマホ）の鋳型を先に整える。まず経路を通し、意味の整理は次段へ。*
