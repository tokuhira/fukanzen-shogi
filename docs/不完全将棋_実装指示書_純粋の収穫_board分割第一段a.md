# 不完全将棋 実装指示書 — 純粋の収穫（board.js 分割 第一段a：純粋 JS を ES モジュールへ）

> 対象実行者: Claude Code（Sonnet 5 または Haiku 4.5）
> 前提: 配布 v0.11.2（board.js 分割 第〇段＝盤面解釈を核へ返す、まで着地。`web/position-view.js` と初の web テスト＝vitest が据わっている。CI 門番は Rust の fmt/clippy/test＋server の typecheck/test＋web の vitest を守る）。関連する現物:
> - `web/board.js`（1496 行）に、DOM にも Wasm にも可変モジュール状態にも触れない**純粋関数と純粋定数**が同居している。第〇段でこれらを割る安全網（CI・web テスト足場）が揃った。
> - 純粋関数（本書で切り出す対象）: `parseUsi`（91–102）・`charToRank`（89）・`countStr`（104–107）・`formatResult`（80–85）・`terminalMessageJa`（270–284）・`renderSvg`（861–940）・`renderHandArea`（942–973）。
> - これらが使う純粋定数: 盤の座標系と字形（`CELL`/`BX`/`BY`/`BW`/`BH`/`SVG_W`/`SVG_H`/`PFS`/`LFS`/`KANJI`/`HAND_ORDER`/`RANK_JA`）、USI（`RANK_CHAR`）、結果語彙（`RESULT_KIND_JA`/`OUTCOME_JA`）。
> - **重要な現物観察**: 盤の座標系（`CELL`/`BX`/`BY`/`BW`/`BH`/`SVG_W`/`SVG_H`/`PFS`）と字形（`KANJI`/`HAND_ORDER`）と `countStr` は、**描画（`renderSvg`/`renderHandArea`）とヒットテスト（`svgCoords`/`getBoardSquare`/`getHandPieceAt`——board.js に残る）の両方が使う**。持ち駒のクリック判定がラベル幅（`(KANJI[k]+countStr(cnt)).length * PFS`）を要するため。これは「盤の座標系」という一つの共有関心事であり、描画専用ではない。
> 関連文書: `不完全将棋_実装指示書_盤面解釈を核へ返す_board分割第零段`（第〇段。本書は §7 申し送りの第一段を二分した前半）、`不完全将棋_実装指示書_足場の整備`、`不完全将棋_バックログ_伏線と未決`。
> 性格: 第〇段が「割る前に核へ返す」だったのに対し、第一段aは**「割らずに済まない純粋部分を、盤の god ファイルから静かに持ち出す」**。**Rust には一切触れない・Wasm は再ビルドしない**（純粋 JS のコード移動のみ）。**消費者から見た挙動は完全に不変**（盤は同一に描画され、ルール・プロトコル・棋譜書式も無変更）。段を細かく刻むのは、実装を小さな独立ユニットに分け、どのモデルでも取りこぼしにくくするため——各 § はそれぞれ単独で完了・検証できる。

---

## 0. 目的と範囲

- **作るもの**（新規 ES モジュール 4 本＋各テスト。すべて純粋 JS、Wasm 非依存）:
  1. `web/usi.js` — `parseUsi`・`charToRank`・定数 `RANK_CHAR`。
  2. `web/result-view.js` — `formatResult`・`terminalMessageJa`（`maxTurns` を**引数化**）・定数 `RESULT_KIND_JA`・`OUTCOME_JA`。
  3. `web/geometry.js` — 盤の座標系・字形の共有定数（`CELL`〜`RANK_JA`）＋ `countStr`。描画とヒットテストが共に読む単一の正本。
  4. `web/board-view.js` — `renderSvg`・`renderHandArea`。`geometry.js` から定数を import。
- **位置づけ**: board.js 分割の**第一段a（純粋の収穫）**。第〇段の申し送り §7 が「純粋 JS の切り出し＋Wasm-in-node テスト足場」を一段に束ねていたのを、**純粋部分（Wasm 足場を要さない・価値の芯）**と**Wasm-in-node 足場（第一段b へ後回し）**に二分した前半。renderSvg（80 行・視覚回帰が最も潜む面）を含む純粋群をテスト網に載せ、board.js を縮める。
- **作らないもの（＝後段・理由つき）**:
  - **Wasm-in-node のテスト足場**: 本書の 4 モジュールは全て純粋で、**素の vitest だけでテストできる**（Wasm を node で読む必要がない）。node で Wasm を読む足場は、それを本当に必要とする最初のユニットが現れたとき組む。→ **第一段b**。
  - **`usiToText`（109–113）**: 3 行の糊（`wasmLegalActions`＋`wasmJaNotation` を呼ぶだけ。日本語棋譜の実体は notation-wasm にあり Rust テスト済み）。テストには Wasm-in-node 足場が要るので、足場を組む第一段bまで **board.js に残す**。
  - **ヒットテスト・入力・DOM・描画呼び出し**（`svgCoords`・`getBoardSquare`・`getHandPieceAt`・`handleSvgClick`・`render` 等）: 状態や DOM に触れる。**board.js に残す**（第二段で状態を持つ分割の対象）。ただし本書で共有定数を `geometry.js` へ移すため、これらは `geometry.js` から定数を import するよう書き換える（§4）。
  - board.js の構造的分割（状態を持つ分割＝第二段）。

---

## 1. 境界の原則（どこで割るか）

第〇段は「ゲームの真実（SFEN の意味）は核へ、ピクセルは盤に」だった。第一段aは盤の中を、**純粋（入力に対して出力が決まり、DOM も Wasm も可変状態も触れない）か否か**で割る。純粋なら持ち出してテストできる。四つの純粋モジュールは互いに素直な依存で並ぶ:

- `usi.js`（USI 文字列の解釈）— 依存なし。
- `result-view.js`（結果 → 日本語表示文字列）— 依存なし。
- `geometry.js`（盤の座標系・字形・数え字）— 依存なし。**描画とヒットテストの共有基盤。**
- `board-view.js`（盤の SVG 描画）— `geometry.js` に依存。

`geometry.js` を `board-view.js` と分けるのは、**盤の座標系が描画とヒットテストの共有関心事**だから（前提の重要な現物観察）。ここを独立させておくと、第二段で入力層（`getBoardSquare` 等）を board.js から割り出すとき、入力層が描画モジュールに依存せずに済む（依存の向きが正しくなる）。＝**既にコード内に潜在している「座標系」という概念を、名前のあるモジュールとして顕在化させる**。過剰な分割ではなく、実在する継ぎ目を彫り出す。

> **設計上の判断（作り手の裁量あり）**: `geometry.js` を独立させず `board-view.js` に定数を同居させれば新規ファイルは 3 本で済む。その場合 board.js のヒットテストが `board-view.js`（描画モジュール）から定数を import することになり、入力→描画という**第二段で解く必要のある逆向きの依存**を一時的に作る。本書は 4 本（`geometry.js` 独立）を既定とする。3 本に畳みたい場合はこの § と §3・§4 の `geometry.js` を `board-view.js` へ読み替える。

---

## 2. モジュール 1・2（依存なし・自己完結）

### 2-1. `web/usi.js`

board.js の `charToRank`（89）・`parseUsi`（91–102）・定数 `RANK_CHAR`（54）を**そのまま**移す（ロジック無変更）。

```js
// USI 文字列の純粋な解釈。DOM・Wasm・可変状態に非依存（board.js 分割 第一段a）。

const RANK_CHAR = 'abcdefghi';

export function charToRank(c) { return RANK_CHAR.indexOf(c) + 1; }

export function parseUsi(usi) {
  if (usi[1] === '*') {
    return { usi, isDrop: true, kind: usi[0], to: [parseInt(usi[2]), charToRank(usi[3])], promote: false };
  }
  return {
    usi,
    isDrop:  false,
    from:    [parseInt(usi[0]), charToRank(usi[1])],
    to:      [parseInt(usi[2]), charToRank(usi[3])],
    promote: usi.length === 5,
  };
}
```

- board.js からこれら（`charToRank`・`parseUsi`・`RANK_CHAR` の定義）を削除し、先頭の import 群へ `import { parseUsi } from './usi.js';` を足す（board.js が使うのは `parseUsi` のみ——512・826・827 行。`charToRank` は `parseUsi` 内部専用なので export はするが board.js では import 不要）。

### 2-2. `web/result-view.js`

`formatResult`（80–85）・`terminalMessageJa`（270–284）・定数 `RESULT_KIND_JA`（64–72）・`OUTCOME_JA`（73–78）を移す。**`terminalMessageJa` だけ一点変更**——現在モジュールグローバルの可変 `maxTurns`（board.js の `let`、`init()` 後に一度 `wasmMaxTurns()` で代入）を掴んでいるので、**第3引数 `maxTurns` として受け取る**純粋関数にする。

```js
// 結果・終局の日本語表示文字列（presentation / i18n）。純粋（board.js 分割 第一段a）。

const RESULT_KIND_JA = {
  mate: '詰み', king_death: '玉が取られた', swap_draw: '両玉相討ち',
  sennichite: '千日手', resign: '投了', unfinished: '未完', other: 'その他',
};
const OUTCOME_JA = {
  sente_wins: '先手の勝ち', gote_wins: '後手の勝ち', draw: '引き分け', none: '',
};

export function formatResult(result) {
  const kindJa = RESULT_KIND_JA[result.kind] || result.kind;
  if (result.outcome === 'none') return kindJa;
  const outcomeJa = OUTCOME_JA[result.outcome] || result.outcome;
  return `${outcomeJa}（${kindJa}）`;
}

// maxTurns はモジュールグローバルを掴まず引数で受ける（純粋化）。
export function terminalMessageJa(kind, outcome, maxTurns) {
  if (kind === 'mate') {
    if (outcome === 'gote_wins')  return '後手の勝ち（先手が着手不能）';
    if (outcome === 'sente_wins') return '先手の勝ち（後手が着手不能）';
    if (outcome === 'draw')       return '引き分け（両者着手不能）';
  }
  if (kind === 'king_death') {
    if (outcome === 'gote_wins')  return '後手の勝ち（先手玉が取られた）';
    if (outcome === 'sente_wins') return '先手の勝ち（後手玉が取られた）';
  }
  if (kind === 'swap_draw'  && outcome === 'draw') return '引き分け（両玉相討ち）';
  if (kind === 'sennichite' && outcome === 'draw') return '引き分け（千日手）';
  if (kind === 'max_turns'  && outcome === 'draw') return `引き分け（最長手数・${maxTurns}組手）`;
  return null;
}
```

- board.js からこれらの定義を削除し、`import { formatResult, terminalMessageJa } from './result-view.js';` を足す。
- **唯一の呼び出し元の更新**: board.js の `computeGameOver`（289）を `return terminalMessageJa(term.kind, term.outcome, maxTurns);` にする（board.js のモジュール変数 `maxTurns` を渡す）。`formatResult` の呼び出し元（401・1179）は無変更。

---

## 3. モジュール 3・4（幾何と描画）

### 3-1. `web/geometry.js`

盤の座標系・字形の共有定数と `countStr` を移す。**これらは描画（`board-view.js`）とヒットテスト（board.js に残る `getBoardSquare` 等）の両方が読む単一の正本。** ロジック・値は無変更。

移す定数（board.js 38–53 のうち視覚に関わる分）: `CELL`・`BX`・`BY`・`BW`・`BH`・`SVG_W`・`SVG_H`・`PFS`・`LFS`・`KANJI`・`HAND_ORDER`・`RANK_JA`。移す関数: `countStr`（104–107）。

```js
// 盤の座標系・字形・数え字。描画（board-view.js）とヒットテスト（board.js）が
// 共に読む共有基盤。純粋データ＋純粋ヘルパ（board.js 分割 第一段a）。

export const CELL  = 38;
export const BX    = 6;
export const BY    = 58;
export const BW    = CELL * 9;        // 342
export const BH    = CELL * 9;        // 342
export const SVG_W = BX + BW + 30;    // 378
export const SVG_H = BY + BH + 50;    // 450
export const PFS   = 22;
export const LFS   = 11;

export const KANJI = {
  P:'歩', L:'香', N:'桂', S:'銀', G:'金', B:'角', R:'飛', K:'玉',
  '+P':'と', '+L':'杏', '+N':'圭', '+S':'全', '+B':'馬', '+R':'龍',
};
export const HAND_ORDER = ['R','B','G','S','N','L','P'];
export const RANK_JA    = ['一','二','三','四','五','六','七','八','九'];

export function countStr(n) {
  if (n <= 1) return '';
  return n <= 9 ? RANK_JA[n - 1] : String(n);
}
```

- **board.js に残す定数はここへ移さない**: `INITIAL_SFEN`（31）・`MAX_ARCHIVE_BYTES`（36）・`EVENT_LABEL`（56–61）は純粋関数が使わず board.js の状態管理／描画呼び出し側でのみ使う。**据え置き**（過ぎたるは及ばざる——純粋関数が要る定数だけ持ち出す）。
- `RANK_CHAR` は `usi.js` 側（§2-1）。`RESULT_KIND_JA`/`OUTCOME_JA` は `result-view.js` 側（§2-2）。混同しない。

### 3-2. `web/board-view.js`

`renderSvg`（861–940）・`renderHandArea`（942–973）を**そのまま**移す（ロジック無変更）。使う定数・`countStr` は `geometry.js` から import。

```js
import {
  CELL, BX, BY, BW, BH, SVG_W, SVG_H, PFS, LFS,
  KANJI, HAND_ORDER, RANK_JA, countStr,
} from './geometry.js';

export function renderSvg(pos, overlay) { /* 861–940 をそのまま */ }
function renderHandArea(buf, hand, label, x, y, hl = null, side = 's') { /* 942–973 をそのまま */ }
```

- `renderHandArea` は `renderSvg` からのみ呼ばれるので export 不要（module 内 private）。`renderSvg` のみ export。
- 本文中の定数参照（`CELL`・`KANJI`・`countStr` 等）は import で解決される。ロジックは 1 文字も変えない。

### 3-3. board.js 側の書き換え（幾何の逆流 import）

board.js に残るヒットテスト・描画呼び出しが使う定数を `geometry.js` から import する。board.js から §3-1 の定数定義と `countStr` 定義を削除し、先頭 import 群へ:

```js
import {
  CELL, BX, BY, BW, BH, SVG_W, SVG_H, PFS, KANJI, HAND_ORDER, countStr,
} from './geometry.js';
import { renderSvg } from './board-view.js';
```

- board.js がこれらを使う残存箇所（すべて無変更で import に解決される）: `svgCoords`（698–699: `SVG_W`/`SVG_H`）、`getBoardSquare`（704–706: `BX`/`BY`/`BW`/`BH`/`CELL`）、`getHandPieceAt`（712–717: `PFS`/`BX`/`HAND_ORDER`/`KANJI`/`countStr`）、`handleSvgClick`・`_advanceFromReveal`（744・810・812: `BY`/`BH`）、`render`（1065: `SVG_W`/`SVG_H`／1066: `renderSvg` 呼び出し）。
- **board.js は `LFS`・`RANK_JA` を import しない**（描画専用で board.js の残存コードは使わない。これらは `board-view.js` が geometry.js から import）。上の import リストに `LFS`・`RANK_JA` を入れないこと。

---

## 4. テスト（すべて素の vitest・Wasm 非依存）

`web/test/` に 3 本追加（`position-view.test.js` と同じ流儀）。CI の web ジョブは既に `npm ci && npm test` を回すので**新テストは自動で拾われる。CI 設定の変更は不要**。

- **`web/test/usi.test.js`**: `parseUsi('7g7f')` が `{isDrop:false, from:[7,?], to:[7,?], promote:false}`（`from`/`to` の rank は `charToRank` 経由）、打ち `parseUsi('P*5e')` が `{isDrop:true, kind:'P', to:[5,5]}`、成り `parseUsi('2b2a+')` が `promote:true`（length 5）。`charToRank('a')===1`・`charToRank('i')===9`。
- **`web/test/result-view.test.js`**: `formatResult({kind:'mate',outcome:'gote_wins'})` → `'後手の勝ち（詰み）'`、`outcome:'none'` の投了系で括弧なし。`terminalMessageJa` の全分岐（`mate`×3・`king_death`×2・`swap_draw`・`sennichite`・`max_turns`）。**`max_turns` は `terminalMessageJa('max_turns','draw',500)` → `'引き分け（最長手数・500組手）'`** で maxTurns 引数化を検証。該当なしは `null`。
- **`web/test/board-view.test.js`**: 手書きの `pos`（`{board:Map, handS, handG}`——`position-view.test.js` と同じく小さな Map を直に作る）で `renderSvg(pos, overlay)` を呼び、次を検証:
  - 先手駒は回転なしの `<text>`、後手駒は `transform="rotate(180,...)"` を含む。
  - 成り駒の字形（`+P`→`と`、`+B`→`馬` 等、`KANJI` 経由）。
  - 持ち駒があれば持駒欄にその字形＋数え字（`countStr`）、無ければ `なし`。
  - `overlay.legalDots`（Set）で `<circle>`、`overlay.selectedSquare` で強調 `<rect fill-opacity="0.14">`。
  - **出力の固定（golden）**: 代表局面（数駒＋両持ち駒＋overlay 一つ）で `expect(renderSvg(pos, overlay)).toMatchSnapshot()` を一つ置き、以降の描画差分を機械的に捕える（第二段で SVG が知らぬ間に変わらない錠）。※`renderSvg` の駒描画順は Map の挿入順に従うので、テストの Map は固定順で作る。
- **移行時の等価確認（開発時・一度きり、着地後に削除）**: 第〇段の `parseSfenLegacy` と同じ流儀で、切り出し前の `renderSvg` を `renderSvgLegacy` として board.js に一時的に残し、代表 `pos` 群（初期・成り含む中盤・持ち駒あり/なし・各種 overlay）で `renderSvg(pos,ov)`（新・board-view.js）と `renderSvgLegacy(pos,ov)` が**文字列一致**することを確認してから legacy を削除する。純粋なコード移動ゆえ一致するはず——一致しなければ移動時のミスの検出になる。

---

## 5. 受け入れ（手動・視覚）

- `cd web && npm test` が緑（既存 `position-view` ＋新規 3 本）。
- 盤が従来と**視覚的に同一**に描画される: 初期局面・成りを含む中盤・持ち駒あり／後手番の回転・選択強調・合法手の墨点・持駒欄。
- クリック選択・再生（← →）・分岐・オンライン対戦・観戦が回帰しない（座標系を `geometry.js` へ移したので、特にクリック→マス判定と持ち駒クリックを確認）。
- 終局メッセージが全種別で従来通り（特に最長手数の「500組手」表示＝`maxTurns` 引数化の実地確認）。

---

## 6. 版の刻み

- **製品挙動は不変**。かつ**Rust に一切触れず Wasm も再ビルドしない**（純粋 JS のコード移動のみ——第〇段より更に blast radius が小さい）。整備・挙動不変リファクタと同じ扱いで、**配布版の bump は不要**（既定は据え置き v0.11.2）。判断はお任せ。**RULE 0.6・PROTOCOL 4・アーカイブ書式 1 はすべて不変**、`web/package.json` の version も据え置きでよい。

---

## 7. 申し送り（後段）

- **第一段b（Wasm-in-node のテスト足場）**: node で `.wasm` を `fs.readFileSync` → `init(bytes)` で読む足場を組み（既定の `fetch(new URL(...))` 経路には触れずビルドレス維持）、`usiToText`（notation-wasm への糊）等の Wasm を要する薄い層をテストへ載せる。**それを本当に必要とするユニットが現れたとき**着手する（第二段の controller 検証と同時になる公算が大きい）。過ぎたる回避で、空手形の足場は先に組まない。
- **第二段（状態を持つ分割）**: model / view / controller を大域変数から小さな状態モジュールへ。入力層（`svgCoords`・`getBoardSquare`・`getHandPieceAt`・`handleSvgClick`）を割り出すとき、本書で独立させた `geometry.js` を描画と共有して読む（入力が描画に依存しない継ぎ目が既にある）。第〇段・第一段aでテスト網が揃ってから。
- **docs 更新**（作り手がコミット）: `docs/README.md` の完了記録表に本書を一行、`バックログ D` の board.js 項目へ「第一段a 済／第一段b・第二段 待ち」を刻む。

---

## 8. 不変の原則（本実装が守るもの）

1. **純粋か否かで割る**: DOM・Wasm・可変状態に触れない関数と定数だけを持ち出す。触れるもの（ヒットテスト・描画呼び出し・状態）は board.js に残す。
2. **座標系は一つの正本**: 描画とヒットテストが共有する盤の幾何は `geometry.js` に一本化し、双方がそこを読む（入力が描画に依存しない継ぎ目を作る）。
3. **ロジックは 1 文字も変えない**: 純粋なコード移動。唯一の変更は `terminalMessageJa` の `maxTurns` 引数化（グローバル依存を断つための純粋化）。
4. **Rust に触れず Wasm を再ビルドしない・ビルドレス維持**: 本書は純粋 JS のみ。Wasm-in-node 足場も組まない（第一段b へ）。
5. **段階的・最小・単独検証可能**: 4 モジュールは独立に切り出し・テストできる。必要なものだけ持ち出す（自明でない `usiToText` は足場が要るので残す）。挙動不変ゆえ版 bump は任意。

---

*board.js 分割の第一段a——純粋の収穫。DOM も Wasm も可変状態も掴まない純粋関数と純粋定数（USI 解釈・結果の日本語化・盤の座標系・SVG 描画）を、盤の god ファイルから四つの ES モジュールへ静かに持ち出す。描画とヒットテストが共有する座標系は `geometry.js` に一本化し、第二段で入力層を割るときの継ぎ目を先に彫っておく。`terminalMessageJa` はグローバル `maxTurns` を引数化して純粋にする。renderSvg を golden テストで錠し、以降の視覚回帰を機械的に捕える。Rust には触れず、Wasm は再ビルドせず、ビルドレスのまま、製品挙動は完全に不変。純粋か否かで割り、座標系は一つの正本に。*
