# 不完全将棋 board.js view 純粋化アーク 総括（View-1〜View-3）

*この文書は、`web/board.js` の `render()` が抱えていた「`state` → 表示値」の導出ロジック（ラベル・ボタン・overlay・cursor）を純粋関数へ抜き出し、`web/view-model.js` として結晶化させた一連の実装（View-1〜View-3、web `?v=` 0.12.3→0.12.5、配布据え置き）の総括である。バックログの「現在地」が持っていた完了記録をここへ綴じる。段ごとの詳細は各実装指示書（`archive/implementation/board-js-view-purification/`）にあり、ここは道のり・確立した設計・残された次の畝を粗く俯瞰する地図。*

---

## 0. このアークが始まった地点と、達成したこと

**始点**: board.js 分割アーク（第〇段〜第三段b-3）で神ファイルは 1531→1220 行まで縮み、純粋モジュール 10 本・テスト 57 件が既に確立していた。しかし `render()` 自体（当時 840 行超）はまだ「`state` を読んで DOM へ書く」処理そのものの中に、ラベル・ボタン・overlay・cursor の導出ロジックがベタ書きされたままだった。ルール変更（終局種別の追加等）が触るのは主にこの表示導出部分であり、ここが純粋関数でない限り「ルール変更の表示追加を安全にテストする」ことができなかった。

**終点（現在）**: `render()` のあらゆる「`state`（＋盤面から導く終局メッセージ `gameOverMsg`）→ 表示値」の導出が `web/view-model.js` の純粋関数群（`labelView`・`buttonView`・`overlay`・`cursorInteractive`、そして合成 `viewModel`）へ移った。`render()` は「wasm 依存の `pos`（`parseSfen`）・`gameOver`（`getGameOverMsg`）を作り、`viewModel(state, gameOver)` を一度呼び、返ってきた表示値を DOM へ流すだけ」の薄い殻になった。`board.js` は 1263→1130 行、`view-model.js` は新設 183 行、web テストは 57→118 件（view-model 単体で 61 本）。**Wasm は引数注入**の原則を貫いたので、`view-model.js` は wasm 非依存——node（vitest）でビルドレスのままテストできる。

**確立した設計パターン**（board.js 分割アークの原則を「view」という切り口で再確認・完遂）:
- **純粋な芯を抜き、殻に薄いラッパ**: `render()` から計算を追い出し、DOM 書き込みだけを残す。board.js 分割アークが確立したパターンの、`render()` 自身への適用。
- **Wasm は引数注入**: `parseSfen`（`wasmPositionView` 経由）と `getGameOverMsg`（`evaluate_terminal` 経由＋メモ化）は wasm 依存なので純粋モジュールに持ち込まず、`render()` が計算した結果を `viewModel(state, gameOverMsg)` へ注入する。`board-view.js`（`revealOverlay`/`inputOverlay`/`renderSvg`）は元々 wasm 非依存だったので、`overlay`/`revealOverlay`/`inputOverlay` は view-model.js が import してよい（`renderSvg` だけは DOM 直前なので render に残る）。
- **芽を育てる**: 既にあった `navView()`（`nav.js` の `navReduce` 用スナップショット、`{phase, cursor, pliesLen, onlineMode, onlineGameOver}` を返す純粋関数）が、このアークの「精神的な前例」だった。`navView` 自体は別の消費者（`navReduce`）を持つため触らず、render 用に**新しい・より完全な `viewModel`** をその隣に育てた——「新しい平行構造を作らない」という言葉の実際の意味は、`navView` を無理に拡張することではなく、同じ設計パターンを繰り返すことだった。
- **一字一句移す・網羅列挙より挙動保存を優先**: 3段とも「現行ロジックをそのまま移す」ことに徹し、ロジックの改善やバグ修正は一切行わなかった（例: View-2 で発見した「前へボタンは入力中でも無効にならず、押すと入力をキャンセルする」という一見非直感的な仕様も、そのまま保存した）。golden snapshot テストは「正しい仕様を書く」ためでなく「今の挙動を固定する」ために書いた。

---

## 1. 純粋モジュールの API（`web/view-model.js`、View-1〜3 で新設・以後 render が消費するだけ）

| 関数 | 役割 | 導入段 |
|---|---|---|
| `labelView(state, gameOverMsg)` | `phaseText`/`moveText`/`eventText`/`archiveInfo`/`step`/`total` を返す | View-1 |
| `watchPhaseText(state, gameOver)`／`onlinePhaseText(state, gameOver)`／`archiveInfoText(state)` | `labelView` が使うラベルヘルパ（旧 `_watchPhaseText`/`_onlinePhaseText`/`archiveInfoText` を `state` 引数化） | View-1 |
| `buttonView(state, gameOverMsg)` | `next`/`prev`/`resign`/`save`/`startButtonsDisabled`/`leaveWatchHidden` を返す | View-2 |
| `overlay(state)` | `revealOverlay`/`inputOverlay`（`board-view.js`・wasm 非依存）の呼び分けを返す | View-3 |
| `cursorInteractive(state, gameOverMsg)` | 盤の SVG カーソルが `pointer` か否か | View-3 |
| `viewModel(state, gameOverMsg)` | 上記すべてを一つの束に合成する、`render()` が実際に呼ぶ唯一の窓口 | View-3 |

いずれも `state`（＋ `gameOverMsg`）だけを引数に取り、DOM にも wasm にも触れない。`render()` は最終的に `viewModel` だけを import すればよい形に整理された。

---

## 2. 段ごとの道のり（course-grained）

**View-1（ラベルの純粋化・`labelView`）**: `render()` のラベル導出ブロック（`phaseText`/`moveText`/`eventText`/`archiveInfo`/`step`/`total`）を `view-model.js` へ抜いた。`_watchPhaseText`/`_onlinePhaseText`/`archiveInfoText`（いずれも module global `state` を読んでいた）を `state` 引数化し、`EVENT_LABEL` も移設。golden snapshot テスト 32 本（reveal・観戦・オンライン・ローカル・アーカイブ不一致・step/total）。実ブラウザでローカル対局一巡とアーカイブ不一致表示を確認。

**View-2（ボタンの純粋化・`buttonView`）**: `render()` のボタン節（55行、watch/online/local の分岐＋resign/save/開始系/観戦離脱）を `buttonView` へ抜いた。この段で「render 冒頭の `bothReady` 計算が `buttonView` 内部の計算と重複し不要になった」ことに気づき削除——View-1/2 の指示書自身が「ボタンのためだけに計算していたなら不要になり得る」と明記していた、まさにその通りの整理。golden snapshot テスト 25 本追加。実ブラウザで駒選択→両者確定→解決→組手後のボタン状態（テキスト・disabled）を確認し、「前へボタンは入力中でも無効化されず、押すと入力をキャンセルする」という一見バグに見える仕様が実は `goPrev()` の意図した挙動であることをコードで確認してから、テストの期待値を正しく書き直した。

**View-3（overlay/cursor の純粋化・アークの締め）**: 残った `overlay`（reveal/入力中/なし の切り替え）と `cursorInteractive`（SVG カーソルが `pointer` か）を純粋関数へ抜き、`labelView`＋`buttonView`＋`overlay`＋`cursorInteractive` を合成した `viewModel(state, gameOverMsg)` を置いた。`render()` は「`pos`（wasm）・`gameOver`（wasm）を作る→ `viewModel` を一度呼ぶ→ DOM へ流す」という、指示書が最初に描いた目標構造そのものに育ち切った。`board.js` から `revealOverlay`/`inputOverlay`/`labelView`/`buttonView` の直接 import が消え、`renderSvg`／`viewModel` だけが残った。golden snapshot テスト 29 本（`overlay`・`cursorInteractive`・合成 `viewModel` の透過性）を追加し、実ブラウザで overlay のハイライト表示と cursor の `pointer`/`default` 切り替えを確認。

---

## 3. 既知の限界・次の畝

- **種類2（I/O 絡みの分解）は別アーク**——`handleTurnComplete`・`enterWatchMode`・`endOnlineGame` 等、非同期コールバックの状態更新と純粋遷移が混在する箇所を tui の `online.rs` に倣って割る仕事は、このアークの対象外のまま。頑健性（悪意入力への耐性）の摂取点はこちらの三関数であって `render()` ではない——view 純粋化と種類2 は直交する仕事であり、脅威の切迫度が実感で決まったら着手する（過ぎたるは及ばざる）。
- **`navView()` は無変更**——`nav.js` の `navReduce` 用の別消費者であり、このアークの `viewModel` とは目的が異なる。統合や重複排除の対象にしなかった。

## 4. このアークで効いた流儀（次の Opus・実装者へ）

- **「一見バグに見える既存挙動」を安易に直さない**: View-2 で `prev.disabled` の条件式（`!hasInput` を含む）を golden snapshot に固定しようとしたとき、直感に反する結果（入力中でも「前へ」ボタンが有効のまま）が出た。ここで「テストの期待値が間違っている」と決めつけず、`goPrev()` の実装（入力中は「前へ」がキャンセル用途に転用される）を確認してから、テストを実際の——保存すべき——挙動に合わせて書き直した。挙動保存リファクタでは、直感より現物を信じる。
- **指示書自身が指し示す「不要になった重複計算」を見逃さない**: View-2 の指示書は「`bothReady`/`hasInput` がボタンのためだけに render 冒頭で計算されていたなら不要になり得る」と明記していた。移設のたびに render 冒頭の変数を目視で棚卸しし、実際に `bothReady` が死んでいることを確認して削除した——移設は機械的なコピーではなく、コピー元が要らなくなったかどうかまで見届ける仕事。
- **段を跨いだ import の整理は最後にまとめて**: View-1/2 の時点では `board.js` はまだ `labelView`/`buttonView` を直接呼んでいたので個別に import していたが、View-3 で `viewModel` が唯一の呼び出し窓口になった時点で、板の直接 import（`labelView`・`buttonView`・`revealOverlay`・`inputOverlay`）をまとめて `viewModel` 一本に整理した。各段の途中で先回りして最終形の import に寄せようとせず、各段はその段の責務だけに閉じ、整理は本当に不要になった時点で行う。
- **合成の透過性をテストで固定する**: View-3 では `viewModel` が `labelView`/`buttonView`/`overlay`/`cursorInteractive` の個別呼び出しと一致することを golden snapshot で確認した。合成関数が「部品の単純な束ね」であることをテストで保証しておくと、将来 `viewModel` だけを見て「部品のどれかを見忘れていないか」を確認する必要がなくなる。

---

*`render()` はもう「盤面と終局を確かめながら DOM を書き換える」関数ではない。`view-model.js` が「`state` は今どんな盤上の物語を語っているか」を答え、`render()` はその答えを聞いて DOM へ伝えるだけの、聞き役に徹する薄い殻になった。ルール変更で終局の種類が増えても、投了の言い回しが変わっても、直すべき場所は `view-model.js` の中だけで、golden snapshot がそこを守ってくれる。残る本丸は種類2（I/O 分解）——脅威の実感が「これだ」と言うのを待てばいい。*
