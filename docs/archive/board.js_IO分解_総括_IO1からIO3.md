# 不完全将棋 board.js I/O 分解アーク 総括（IO-1〜IO-3）

*この文書は、`web/board.js` の I/O 絡みの三関数（`handleTurnComplete`・`enterWatchMode`・`endOnlineGame`）に埋め込まれていた純粋な判断ロジックを `web/reducers.js` の純粋 reduce へ抜き出した一連の実装（IO-1〜IO-3、web `?v=` 0.12.5→0.12.8、配布据え置き）の総括である。バックログの「現在地」が持っていた完了記録をここへ綴じる。段ごとの詳細は各実装指示書（`archive/implementation/board-js-io-decomposition/`）にあり、ここは道のり・確立した設計・残された次の畝を粗く俯瞰する地図。*

---

## 0. このアークが始まった地点と、達成したこと

**始点**: board.js 分割アーク（第〇段〜第三段b-3）と view 純粋化アーク（View-1〜View-3）で、「state 器→update→reduce」のパターンと「render() の表示導出を純粋関数へ」のパターンは既に確立していた（`reducers.js` に `resetOnlineReduce`・`hotseatConfirmReduce`、`view-model.js` に `viewModel` 等）。しかし残った三関数（`handleTurnComplete`・`enterWatchMode`・`endOnlineGame`）は、非同期 I/O（WS 送信・`connectSpectate` コールバック・`setTimeout`・記録係待ち）と純粋な判断ロジック（投了判定・meta マッピング・終局 patch）が縒れたままだった。埋め込まれた分岐（投了三態×視点、観戦の追いつき/再戦、記録係食い違い/archived、終局 patch）は純粋なのにテストできず、頑健性（悪意ある相手・不正な reveal への耐性）の摂取点もこの三関数だった。

**終点（現在）**: 三関数すべてが「純粋遷移（`reducers.js` の reduce）＋薄い I/O」に揃った。

- `handleTurnComplete`（IO-1）: 投了判定を `turnCompleteDecision(senteUsi, goteUsi, onlineSide)` へ抜き、合法性検証（wasm・安全弁）・通常 append・render は殻に残した。
- `enterWatchMode`（IO-2）: `_metaToLoadedMeta` を `metaToLoadedMeta` として移設（先頭 `_` を外し export）、記録係イベントの `archivedLink` 導出を `archivedLinkFor(id, archiveUrl)`（archiveUrl 注入）として新設した。`connectSpectate`・`loadPlies`・`resetToNew`・`watchAppendTurn`・`render` の I/O は無変更。
- `endOnlineGame`（IO-3）: 終局 patch を `endGameReduce(msg)` へ抜いた。`currentResult`（wasm）・`sendSpectateResult`・`isRecording` 分岐・`sendRecordTestimony`・記録係待ちの 5 秒保険タイムアウト・`disconnectOnline` という本質的 I/O の effect 列は、末尾一行以外まったく無変更で殻に残した。

`web/reducers.js` は `resetOnlineReduce`・`hotseatConfirmReduce`（既存）に加え `turnCompleteDecision`・`metaToLoadedMeta`・`archivedLinkFor`・`endGameReduce`（新設）を持つ。web テストは 118→131 件（`reducers.test.js` に table テスト 13 本追加）。web `?v=` は 0.12.5→0.12.8、配布は v0.12.3 のまま据え置き。

---

## 1. 純粋 reduce の API（`web/reducers.js`、IO-1〜3 で新設・移設）

| 関数 | 役割 | 導入段 |
|---|---|---|
| `turnCompleteDecision(senteUsi, goteUsi, onlineSide)` | 投了三態（先手/後手/両者）×視点 → msg/outcome/resultOverride、非投了 → `{kind:'live'}` | IO-1 |
| `metaToLoadedMeta(version, result)` | 版タプルと結果から loadedMeta を組む（null 版ガード・result 既定値）。`_metaToLoadedMeta` を移設 | IO-2 |
| `archivedLinkFor(id, archiveUrl)` | アーカイブ id → リンク情報（`archiveUrl` 注入・id 無しは null） | IO-2 |
| `endGameReduce(msg)` | オンライン対局の終局 patch（`resetOnlineReduce` の終局版） | IO-3 |

いずれも wasm・DOM に非依存。node（vitest）でビルドレスのままテストできる。

---

## 2. 段ごとの道のり（course-grained）

**IO-1（`handleTurnComplete` の投了判定）**: 投了判定ブロック（`sResign`/`gResign`・msg/outcome の分岐・`resultOverride`）を一字一句 `turnCompleteDecision` へ移し、`handleTurnComplete` を「reduce を呼ぶ→投了なら `resultOverride` を立てて `endOnlineGame`／非投了なら合法性（wasm 安全弁）→通常 append（wasm）→render」の薄い殻にした。table テスト 6 本（投了三態×視点＝6経路＋非投了）。実ブラウザで本番 DO 相手に、通常手・先手投了・後手投了・両者投了の全4シナリオを検証し、自分/相手それぞれの視点でメッセージが従来どおりであることを確認した。

**IO-2（`enterWatchMode` の純粋部）**: `_metaToLoadedMeta`（onInit・onMeta で共有されていた既存の純粋関数）を先頭 `_` を外して `reducers.js` へ移設し、`onRecordDisagreement`/`onArchived` の `archivedLink` 導出（`id ? {id, url: archiveUrl(id)} : null` の反復パターン）を `archivedLinkFor(id, archiveUrl)` として新設・共通化した。`onStatus`/`onRecordConfirmed`/`onResult` 等のリテラル patch は「論理を持たない」として reduce 化しなかった（過ぎたるは及ばざる）。table テスト 5 本。実ブラウザで、対局途中から観戦開始したときの追いつき（onInit・null result → 既定値パス）、進行中の onTurn 伝播、投了→記録係証言一致→アーカイブ確定が観戦者側にも `archivedLinkFor` 経由で正しく届くこと（onArchived）、同じ部屋での再戦時に観戦側の局面・記録表示が正しくリセットされること（onMeta）を確認した。

**IO-3（`endOnlineGame` の終局 patch・アークの締め）**: 末尾の `update({onlineGameOver, onlineEndMsg, onlineCommitted:false, onlineWaiting:false})` を一字一句 `endGameReduce(msg)` へ抜き、`update(endGameReduce(msg))` に置き換えた。変更は本当にその一行のみ——`currentResult`（wasm）・`sendSpectateResult`・`isRecording` 分岐・`sendRecordTestimony`・記録係待ちの 5 秒保険タイムアウト（`_pendingRecordDisconnect`）・`disconnectOnline` という effect 列とそのコメント（rationale）は完全に無変更のまま殻に残した。table テスト 2 本。実ブラウザで、記録係なし（即切断）・記録係あり（証言一致→保険タイムアウトを待たずアーカイブ確定→切断）の両シナリオを本番 DO で確認し、終局メッセージ・切断タイミング・記録表示が従来どおりであることを確かめた。

---

## 3. 既知の限界・次の畝

- **`board.js` の残り本丸（view 純粋化＋I/O 分解）はこれで尽きた**。次にルール変更（終局種別の追加・投了の言い回しの変更等）に着手するときは、`view-model.js`（表示導出）と `reducers.js`（状態遷移の判断）という、golden snapshot／table テストで守られた地盤の上で進められる。
- **`isRecording` 分岐そのものは reduce 化していない**——証言送出＋待ち vs 即切断を分ける、effect を選ぶための I/O 判断であり、純粋な状態 patch ではないため（IO-3 指示書 §0「作らないもの」）。分岐自体を将来 reduce 化する価値が出るかは未知数のまま残す。
- **`onStatus`/`onRecordConfirmed`/`onResult` 等のリテラル patch は意図的に reduce 化していない**——論理を持たない代入は reduce にしても得るものがない（過ぎたるは及ばざる）という判断を、IO-2 で明示的に下した。

## 4. このアークで効いた流儀（次の Opus・実装者へ）

- **本質的 I/O は捻じらない**: 三段とも、wasm 呼び出し（`turnActionsAreLegal`・`usiToText`・`currentResult`・`buildArchiveText`）と真の I/O（WS 送信・`disconnectOnline`・`setTimeout`）は一切純粋化を試みなかった。純粋なのに埋まっていた判断だけを抜く、という一貫した線引きが、各段を「小さく・検証しやすく」保った。
- **前段が着地してから次段の指示書を書く**: アーク概観が明記したとおり、IO-2・IO-3 の指示書は前段のコミット（`f3e9305`・`2b5e14a`）を実地に読み直してから書かれた。行番号や関数の現物を確認してから指示書を作る姿勢が、指示書と実装の齟齬をゼロに保った。
- **移設は一字一句、挙動保存を最優先**: `_metaToLoadedMeta`→`metaToLoadedMeta`、投了判定ブロック、終局 patch のいずれも、ロジックの改善や整理を一切行わず、そのまま移した。判断ロジックの正しさを疑う余地を作らないための規律。
- **table テストは埋め込みで届かなかった分岐を拾うためにある**: `npm test` が緑になることは「抽出した純粋ロジックが正しい」ことの証明にしかならず、`board.js` 自身の配線（import・呼び出し箇所）の正しさは実ブラウザ・実本番 DO でしか確認できない——このアーク中、毎段この二段構えの検証（vitest＋Playwright 実地）を崩さなかった。

---

*三関数はもう「非同期コールバックと状態更新が縒れた I/O のかたまり」ではない。`reducers.js` が「この入力なら状態はこうなる」を答え、`handleTurnComplete`・`enterWatchMode`・`endOnlineGame` はその答えを聞いて I/O を行うだけの、聞き役に徹する薄い殻になった。ルール変更で投了の言い回しが増えても、観戦イベントの種類が増えても、直すべき場所は `reducers.js` の中だけで、table テストがそこを守ってくれる。board.js の地盤づくりはここでひと区切り——次はルール変更のアイデアへ、実感が向くときに。*
