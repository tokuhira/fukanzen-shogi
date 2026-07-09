# 不完全将棋 実装指示書 — 第三段a：状態を単一の `state` 器へ集約する（読み取りの集約）

> 対象実行者: Claude Code（Sonnet 5 を推奨。**本段は board.js のほぼ全行に触れる唯一の段**——機械的だが範囲が広く、挙動不変の担保が要。Haiku 単独より Sonnet が安全）。
> 前提: 配布 v0.11.2 / web `?v=`0.11.7（board.js 分割 第二段c まで着地。web テスト 42 件・純粋モジュール 7 本〔usi/result-view/geometry/board-view/notation-view/game-record/move-input〕・golden snapshot が据わっている）。
> 関連する現物（すべて実地で確認済み）:
> - board.js のモジュールスコープ可変状態は **30 変数**。宣言は 77–128 行に固まっている（`cursor`・`sfens`・`events`・`phase`／`inputStep`・`pendingSente`・`pendingGote`・`selectedFrom`・`legalTargets`・`promotionPending`／`legalCache`・`gameOverCache`／`versionTuple`・`resultOverride`・`loadedMeta`・`maxTurns`／`onlineMode`・`onlineSide`・`onlineCommitted`・`onlineGameOver`・`onlineEndMsg`・`onlineWaiting`・`onlineWaitingMsg`／`spectateToken`・`recordInviteAsked`・`recordStatusText`・`archivedLink`・`_pendingRecordDisconnect`／`watchMode`・`watchStatusText`）。
> - これらは**すべて board.js 内に閉じている**（export されておらず、online.js は状態を共有せず callbacks の関数呼び出しで連携）。よって本段の変換は board.js 一枚で完結し、他モジュールへ波及しない。
> - `const kifu = { plies: [] }`（77）は**本段では据え置く**（第三段b で `state` へ吸収）。`game-record.js` の橋渡し（`setRecord`/`currentRecord`）が `kifu.plies` を触るため、ここを動かすと「更新経路の整理」（第三段b の領分）に踏み込む。器作りと経路整理を混ぜない。
> - **機械変換の危険箇所は洗い出し済み**（実地で変換スクリプトを試作し、全 42 テスト緑・構文 OK を確認済み）。危険は三種のみ:
>   1. `gameOverCache = { cursor: -1, msg: null }`（5 箇所）の**キー `cursor:`** は状態変数の cursor ではない → 触らない。
>   2. `gameOverCache.cursor`（187）・`legalCache.sfen`（426）の**ドット後のプロパティ名** → 触らない（`gameOverCache` 自体は `state.gameOverCache` になるので `state.gameOverCache.cursor` が正）。
>   3. `svg.style.cursor`（817）の **CSS プロパティ cursor** → 触らない（別物）。
>   分割代入の左辺・同名ローカル/引数の衝突は**ゼロ**（確認済み）。複合代入は `cursor++`/`cursor--`（3 箇所、`_advanceFromReveal`・`goNext`・`goPrev`）のみ → `state.cursor++` で正しく変換できる。
> 関連文書: `不完全将棋_実装指示書_overlay計算を純粋化して盤へ寄せる_board分割第二段c`、`不完全将棋_実装指示書_棋譜コアの遷移を純粋化_board分割第二段a`（`setRecord`/`currentRecord` の初出）、`不完全将棋_バックログ_伏線と未決`。
> 性格: 第三段a は**「散在する 30 の状態変数を、単一の `state` オブジェクトへ機械的に移す。読む側が『状態一つ』を受け取れる器を作る」**。これは読み書き二段作戦の**読み取り集約**。**更新経路は変えない**——`cursor = 5` が `state.cursor = 5` になるだけ。意味は 1 ミリも変えない。島をまたぐ更新の整理・`kifu` の吸収は第三段b（書き込み集約）へ。Rust に触れず Wasm 再ビルドなし。製品挙動は完全に不変。行番号は v0.11.7 の board.js 基準。

---

## 0. 目的と範囲

- **作るもの**:
  - board.js の 30 状態変数を単一の `const state = { … }` へ集約。全参照・全代入を `state.<名>` へ書き換える（機械的・意味不変）。
  - 挙動不変の担保: 既存 42 テスト（特に golden snapshot）が緑のまま。
- **位置づけ**: board.js 分割の**第三段a（読み取り集約）**。view の純粋関数（第二段c の overlay 等）へ「状態スナップショット」を渡せる器を用意する。これが本丸＝更新経路一本化（第三段b）の土台になり、集約後は「同じ `state` → 同じ描画」をテストで守れるようになる。
- **作らないもの（＝理由つき）**:
  - **更新経路の変更・reducer 化**: `state.x = v` の直接代入のまま。単一更新点への集約は第三段b。本段で経路に触ると範囲が二重に膨らむ。
  - **`kifu` の `state` への吸収**: 据え置き（`setRecord`/`currentRecord` を触ると第三段b の領分）。`state` と `kifu` が一段だけ共存するのは許容。
  - **状態の島分け（`state.online.*` 等のネスト）**: 本段は**フラット**（`state.onlineMode` 等）。ネストは参照の書き換え規則を複雑にし、機械変換の安全性を下げる。島構造が要るなら第三段b 以降で。過ぎたるは及ばざる。
  - **キャッシュ（`legalCache`/`gameOverCache`）の扱いの変更**: これらも `state` に入れるが（`state.legalCache` 等）、内部構造・使い方は無変更。

---

## 1. `state` オブジェクトの定義

宣言が固まっている箇所（77–128、`const kifu` を除く 30 変数の `let`）を削除し、単一の `const state` に置換する。初期値は現在の宣言のものをそのまま移す。**`kifu` は隣に据え置く**。

```js
const kifu = { plies: [] };  // 据え置き（第三段b で state へ吸収）

const state = {
  // 棋譜コア
  cursor: 0,
  sfens: [INITIAL_SFEN],   // sfens[i] = position entering turn i
  events: [],              // events[i] = event string from resolving plies[i]
  phase: 'position',       // 'position' | 'reveal'

  // 入力
  inputStep: null,         // null | 'sente' | 'gote'
  pendingSente: null,      // null | { usi, text }
  pendingGote: null,
  selectedFrom: null,      // null | { board:[f,r] } | { hand:kind }
  legalTargets: null,      // null | Map<"f,r", { options:[{usi,promote}] }>
  promotionPending: null,  // null | { options, toSquare }

  // キャッシュ
  legalCache: { sfen: null, sente: null, gote: null },
  gameOverCache: { cursor: -1, msg: null },

  // メタ / 結果
  versionTuple: null,      // { rule, protocol, app }
  resultOverride: null,    // { kind, outcome } | null
  loadedMeta: null,        // 読み込んだアーカイブの ArchiveMeta
  maxTurns: null,          // ルール v0.6 の最長手数（組手）

  // オンライン
  onlineMode: false,
  onlineSide: null,        // 'sente' | 'gote'
  onlineCommitted: false,
  onlineGameOver: false,
  onlineEndMsg: '',
  onlineWaiting: false,
  onlineWaitingMsg: '',

  // 観戦
  watchMode: false,
  watchStatusText: '',
  spectateToken: null,

  // 記録係
  recordInviteAsked: false,
  recordStatusText: '',
  archivedLink: null,
  _pendingRecordDisconnect: false,
};
```

- 元の各宣言に付いていたコメントは `state` の各行へ移すか要約する（情報を失わない）。

## 2. 参照・代入の変換規則（厳密に）

board.js 全体で、30 の状態変数**識別子**を `state.<名>` に置換する。**構文位置を見て**、以下は**変換しない**:

1. **直前が `.` のもの**（プロパティアクセス）: `gameOverCache.cursor` の `.cursor`、`svg.style.cursor` の `.cursor`、`legalCache.sfen` の `.sfen` など。→ ドット後の識別子は触らない。ただし `gameOverCache` 自体（ドットの**前**）は変換対象なので `state.gameOverCache.cursor` になる。
2. **直後が `:` のもの**（オブジェクトのキー）: `{ cursor: -1, msg: null }` の `cursor:` は触らない。
3. **文字列・コメント内**の同名語。

安全な変換の指針（正規表現なら `(?<![.\w])<名>\b(?!\s*:)` で識別子境界＋ドット前除外＋キー除外）:
- `cursor` → `state.cursor`、`cursor++` → `state.cursor++`、`cursor === 0` → `state.cursor === 0`。
- `gameOverCache = { cursor: -1, msg: null }` → `state.gameOverCache = { cursor: -1, msg: null }`（左辺のみ変換、キーは不変）。
- `if (cursor !== gameOverCache.cursor)` → `if (state.cursor !== state.gameOverCache.cursor)`。
- `svg.style.cursor = (phase === 'position' && …)` → `svg.style.cursor = (state.phase === 'position' && …)`（`style.cursor` は不変、`phase` は変換）。

**検証済みの事実**（実地で変換スクリプトを走らせ確認）: この規則で 30 変数を変換し、`state.state.`（二重化）・`state.cursor:`（キー誤爆）・`style.state.cursor`（CSS 誤爆）は一切生じず、全 42 テストが緑・`node --check` が通る。実装者はこの規則に従い、変換後に下記の grep で誤爆ゼロを確認すること。

## 3. `game-record.js` の橋渡しとの整合

第二段a の `setRecord`/`currentRecord` は `sfens`/`events`/`kifu.plies` を触る。本段で `sfens`/`events` は `state` へ入る（`kifu` は据え置き）。よってこの 2 関数のみ、`state` 参照へ更新:

```js
function setRecord(record) {
  state.sfens  = record.sfens;
  state.events = record.events;
  kifu.plies   = record.plies;   // kifu は据え置き
}
function currentRecord() {
  return { sfens: state.sfens, events: state.events, plies: kifu.plies };
}
```

- これは変換規則の自然な帰結（`sfens`→`state.sfens` 等）で、特別扱いではない。`kifu.plies` だけ従来通り。

## 4. 受け入れ（挙動不変の担保が中心）

- `cd web && npm test` が緑（既存 42 件がすべて、**snapshot 差分ゼロ**。新規テストは無し——本段は器の付け替えで新しい振る舞いを足さない）。
- `node --check web/board.js` が通る。
- **誤爆ゼロの機械確認**（実装者が実行）:
  - `grep -n "state\.state\." board.js` → 0 件。
  - `grep -n "style\.state\." board.js` → 0 件（CSS cursor が守られている）。
  - `grep -nE "\{[^}]*state\.(cursor|msg):" board.js` → 0 件（オブジェクトキー誤爆なし）。
  - `grep -nE "(^|[^.])\b(cursor|phase|onlineMode|selectedFrom|watchMode)\b" board.js | grep -v "state\."` → 状態変数の裸参照が残っていないこと（コメント・キー・プロパティを除き 0）。
- ブラウザで**全機能の手触りが従来と同一**: 新規対局・棋譜読み込み・着手選択と確定・同時開示・分岐・アーカイブ保存/読込・オンライン対戦（commit/reveal/投了/切断）・観戦（ライブ追記・リンク）・記録係（招待・綴じ・リンク）・終局判定・棋譜ナビゲーション（← →）・盤/持ち駒クリック・成り選択 UI。**特に**：`svg.style.cursor`（ポインタ形状）が局面で正しく変わること、`gameOverCache` のキャッシュが効くこと（同一 cursor で終局判定が再計算されない）。

## 5. 版の刻み

- **製品挙動は完全に不変・Rust 非関与・Wasm 再ビルドなし**。ただし board.js のほぼ全行に触れる大きな差分。整備・挙動不変リファクタとして配布版据え置き **v0.11.2**、web の `?v=`（`web/package.json`・`web/index.html`）を **0.11.8** へ前進（board.js 全面改変のためキャッシュ確実更新）。**RULE 0.6・PROTOCOL 4・アーカイブ書式 1 不変**。

## 6. 申し送り（第三段b＝書き込み集約＝本丸へ）

- 読み取りの器（`state`）ができた。次は**更新経路の一本化**（第三段b）: 島をまたぐ 10 個の糊関数（`handleTurnComplete`〔online＋メタ＋棋譜〕・`confirmMove`〔online＋入力〕・`_advanceFromReveal`/`goPrev`〔入力＋棋譜〕・`enterWatchMode`〔4 島〕・`_resetOnlineState`/`loadPlies`/`resetToNew` ら）を、意味のある遷移（アクション）として整理する。ここで reducer 的な形が生きる。
- 同時に **`kifu` を `state` へ吸収**（`state.plies`）し、器を完全に一つにする。`setRecord`/`currentRecord` もそこで畳める。
- view が `state` スナップショットを受け取る純粋関数（render() 本体の phaseText/ボタン分岐の純粋化）は、器が一つになった後に。集約前後で「同じ `state` → 同じ描画」をテストで守れる。
- golden snapshot への局面追加（第一段a の申し送り）は render() を触る view 段で。

---

## 7. 不変の原則（本実装が守るもの）

1. **意味を変えない機械変換**: `x = v` → `state.x = v`。更新経路・タイミング・値は一切変えない。読み取りの器を作るだけ。
2. **構文を見て変換する**: ドット後のプロパティ・オブジェクトキー・CSS `style.cursor`・文字列/コメントは触らない。裸の識別子だけを `state.` 化する。
3. **フラットに保つ**: `state.onlineMode` 等の平坦構造。島ネストは入れない（機械変換の安全性優先）。
4. **`kifu` は据え置く**: 器の統合は第三段b。本段は 30 変数のみ。段の純度を保つ。
5. **挙動不変を snapshot と全機能手触りで担保**: 新規テストは足さず、既存 42 件（特に golden snapshot）の緑と誤爆ゼロ grep で守る。Rust に触れず Wasm 再ビルドなし。配布版据え置き、web `?v=` のみ前進。

---

*第三段a——散在する 30 の状態変数を単一の `state` 器へ集める。読み書き二段作戦の読み取り集約。`let cursor` が `state.cursor` になるだけで、更新経路も値も 1 ミリも変えない。危険は三つ（gameOverCache のキー cursor:・.cursor プロパティ・CSS の style.cursor）だけと洗い出し済みで、構文を見る変換ならすべて避けられることを実地で実証した（変換版で全 42 テスト緑・snapshot 差分ゼロ・node --check 通過を確認）。状態は board.js 内に閉じ export もされていないので、変換は一枚の中で完結し他モジュールへ波及しない。`kifu` は据え置き、島ネストも入れず、フラットで最小に保つ——器作りと経路整理を混ぜない。これで view へ「状態スナップショット」を渡す道が通り、本丸＝更新経路の一本化（第三段b）の土台になる。同じ state から同じ描画、を守れる地面をまず均す。*
