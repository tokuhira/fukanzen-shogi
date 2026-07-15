# 不完全将棋 実装指示書 — board.js I/O 分解 IO-2：`enterWatchMode` の純粋部を reducers へ

> 対象実行者: Claude Code（Sonnet 5）
> 前提: IO-1 着地（HEAD `f3e9305`。`reducers.js` に `turnCompleteDecision`）。`enterWatchMode`（board.js・353 行付近）の connectSpectate コールバックは概ね薄い `update({...})` だが、**論理を持つ純粋部**が二つ埋まっている——`_metaToLoadedMeta`（version/result → loadedMeta・null 版ガードと result 既定値）と、記録係イベントの `archivedLink` 導出（`id ? {id, url: archiveUrl(id)} : null`）。この段は**この二つを reducers.js の純粋関数へ抜き、テストを届ける**。connectSpectate・loadPlies・resetToNew・watchAppendTurn・render の I/O は殻に残す。この段は IO-1 より軽い（watch コールバックが薄いため）が、meta マッピングと archivedLink 分岐のカバレッジを取る。挙動保存。web のみ・`npm test`（vitest）で検証。
> 関連する現物（すべて実地で確認済み・HEAD `f3e9305` 基準）:
> - `web/board.js` `_metaToLoadedMeta(version, result)`（343 行付近・**既に純粋**）: `if (!version) return null;` → `{rule, protocol, app, sente:null, gote:null, result: result ?? {kind:'unfinished', outcome:'none'}}`。onInit・onMeta・loadArchive で使用（複数箇所）。
> - `web/board.js` `enterWatchMode(token)`（353-407）の記録係コールバック:
>   - `onRecordDisagreement(idA, idB, id)`（396-401）: `update({recordStatusText:'記録が食い違いました（裁定はされません）', archivedLink: id ? {id, url: archiveUrl(id)} : null})`。
>   - `onArchived(id)`（402-404）: `update({recordStatusText:'記録されました', archivedLink: {id, url: archiveUrl(id)}})`。
>   - `onRecordConfirmed()`（392）: `update({recordStatusText:'記録係: 有効（この対局は書庫へ綴じられます）'})`（リテラル・reduce 化不要）。
> - `archiveUrl`（board.js が online.js から import・26 行）: id → URL の純粋ビルダー。reducers.js へは**注入**する（decouple 維持）。
> - I/O（殻に残す）: `connectSpectate`・`loadPlies`（wasm 再生）・`resetToNew`（状態リセット）・`watchAppendTurn`（wasm）・`render`（DOM）・`update`。
> - `web/test/reducers.test.js`: vitest。
> 関連文書: `不完全将棋_board.js_IO分解アーク_概観と段組`、IO-1 指示書。
> 性格: IO-2 は**「`enterWatchMode` の watch コールバックに埋まった純粋な論理（`metaToLoadedMeta`・`archivedLink` 導出）を reducers.js の純粋関数へ抜き、テストする」**。`metaToLoadedMeta` を reducers.js へ移設（複数箇所の共有ヘルパ）、`archivedLinkFor(id, archiveUrl)` を新設（archiveUrl 注入）。記録係コールバックはこれらを使う薄い形に。connectSpectate・loadPlies・resetToNew・watchAppendTurn・render の I/O は殻に残す。リテラル文字列の patch（onStatus/onRecordConfirmed 等）は reduce 化しない（過ぎたるは及ばざる）。挙動保存。web のみ・`?v=` 前進・配布据え置き。

---

## 0. 目的と範囲

- **作るもの**:
  1. `web/reducers.js`: `metaToLoadedMeta(version, result)` を board.js から**移設**（純粋・複数箇所の共有）。`archivedLinkFor(id, archiveUrl)` を新設（`id ? {id, url: archiveUrl(id)} : null`・archiveUrl 注入）。
  2. `web/board.js`: `_metaToLoadedMeta` の定義を削除し `metaToLoadedMeta` を import して全呼び出し元を差し替え。記録係コールバック（onRecordDisagreement/onArchived）を `archivedLinkFor(id, archiveUrl)` 経由に。
  3. `web/test/reducers.test.js`: `metaToLoadedMeta`・`archivedLinkFor` の table テスト。
  4. web `?v=` 前進。
- **位置づけ**: I/O 分解アークの **IO-2**。watch 経路の純粋論理（meta マッピング・archivedLink 分岐）にカバレッジを届ける。
- **作らないもの（＝理由つき）**:
  - **リテラル patch の reduce 化**: onStatus（`{watchStatusText}`）・onRecordConfirmed（`{recordStatusText: 固定}`）・onResult（`loadedMeta.result` 代入）は論理を持たないリテラル。reduce 化しない。
  - **loadPlies/resetToNew/watchAppendTurn/render/connectSpectate の純粋化**: 本質的 I/O（wasm 再生・状態リセット・DOM）。殻に残す。
  - **onInit の turns→plies マッピング**: 一行 map（trivial）。cursor は loadPlies 後の `state.plies.length`（I/O 依存）なので殻に残す。
  - **`handleTurnComplete`（IO-1 済）/`endOnlineGame`（IO-3）の変更**。

---

## 1. `web/reducers.js` に純粋部を移設・新設

```js
/**
 * 版タプルと結果から loadedMeta を組む。純粋。
 * （board.js の _metaToLoadedMeta を移設。onInit・onMeta・loadArchive で共有。）
 */
export function metaToLoadedMeta(version, result) {
  if (!version) return null;
  return {
    rule: version.rule, protocol: version.protocol, app: version.app,
    sente: null, gote: null,
    result: result ?? { kind: 'unfinished', outcome: 'none' },
  };
}

/**
 * アーカイブ id からリンク情報を組む。id が無ければ null。純粋（archiveUrl 注入）。
 */
export function archivedLinkFor(id, archiveUrl) {
  return id ? { id, url: archiveUrl(id) } : null;
}
```

- `metaToLoadedMeta` は現行 `_metaToLoadedMeta` を一字一句移す（先頭 `_` を外して export）。
- `archivedLinkFor` は `archiveUrl` を注入で受ける（reducers.js を online.js に依存させない）。

## 2. `web/board.js` の差し替え

- **`_metaToLoadedMeta` の定義を削除**し、`import { …, metaToLoadedMeta, archivedLinkFor } from './reducers.js';`。全呼び出し（onInit・onMeta・loadArchive 等）を `metaToLoadedMeta(...)` に差し替え。
- **記録係コールバック**を `archivedLinkFor` 経由に:

```js
    onRecordDisagreement: (idA, idB, id) => {
      update({
        recordStatusText: '記録が食い違いました（裁定はされません）',
        archivedLink: archivedLinkFor(id, archiveUrl),
      });
    },
    onArchived: (id) => {
      update({ recordStatusText: '記録されました', archivedLink: archivedLinkFor(id, archiveUrl) });
    },
```

- onArchived は現行 `{id, url: archiveUrl(id)}`（id 必ず存在）だが、`archivedLinkFor(id, archiveUrl)` でも同一（id 有り → `{id, url}`）。挙動不変。
- onStatus/onRecordConfirmed/onResult/onInit/onMeta の I/O・リテラルは無変更（metaToLoadedMeta 呼び出しの import 差し替えのみ）。

## 3. テスト（`web/test/reducers.test.js` に追加）

- `metaToLoadedMeta(version, result)`:
  - `version = null` → `null`。
  - `version = {rule:'0.6', protocol:5, app:'0.12.3'}`, `result = null` → `{…, result:{kind:'unfinished', outcome:'none'}}`（既定値）。
  - `result = {kind:'mate', outcome:'sente_wins'}` → その result がそのまま。
  - `sente`/`gote` が `null` であること。
- `archivedLinkFor(id, archiveUrl)`（`archiveUrl` はテスト用スタブ `id => 'u/'+id`）:
  - `id = 'abc'` → `{id:'abc', url:'u/abc'}`。
  - `id = null`/`undefined`/`''` → `null`。

## 4. 受け入れ条件

- `web/reducers.js` に `metaToLoadedMeta`（移設）・`archivedLinkFor`（新設・archiveUrl 注入）。いずれも純粋・wasm/DOM 非依存。
- `board.js` から `_metaToLoadedMeta` の定義が消え、全呼び出しが `metaToLoadedMeta` に。記録係コールバックが `archivedLinkFor` 経由。
- **状態遷移が保存**: 観戦で追いつき（onInit）・再戦（onMeta）・記録係確定/食い違い/archived の各表示が従来と同一。ブラウザで観戦（追いつき・再戦・記録係イベント）を目視。
- `npm test`（vitest）緑（`metaToLoadedMeta`・`archivedLinkFor` テスト＋既存無傷）。
- loadPlies/resetToNew/watchAppendTurn/render/connectSpectate・`handleTurnComplete`・`endOnlineGame` は無変更。engine/protocol/tui/server に差分なし。web `?v=` 前進・配布据え置き。

## 末尾要約

`enterWatchMode` の watch コールバックに埋まった純粋な論理を reducers.js へ抜く。`_metaToLoadedMeta`（版/結果→loadedMeta・複数箇所の共有）を `metaToLoadedMeta` として移設し、記録係イベントの `archivedLink` 導出を `archivedLinkFor(id, archiveUrl)`（archiveUrl 注入）へ抜く。記録係コールバックはこれらを使う薄い形に。connectSpectate・loadPlies・resetToNew・watchAppendTurn・render の I/O は殻に残し、リテラル patch は reduce 化しない。table テストで meta マッピング（null 版・result 既定）と archivedLink 分岐（id 有無）を守る。IO-1 より軽い段だが、watch 経路の純粋論理にカバレッジが届く。挙動保存・web `?v=` 前進・配布据え置き。

## 不変の原則

- **純粋論理は reduce・I/O は殻**: meta マッピングと archivedLink 導出は純粋関数へ。connectSpectate・loadPlies・resetToNew・render は殻。
- **Wasm・I/O・env は注入 or 殻**: `archiveUrl` は注入（reducers.js を decouple）。loadPlies（wasm）・resetToNew（状態）は殻。純粋 reduce は node テスト可能。
- **挙動保存**: 観戦・再戦・記録係イベントの状態遷移を保存。table テストで守る。移設は一字一句。
- **リテラルは reduce 化しない**（過ぎたるは及ばざる）: 論理を持たない patch（status/confirmed/result）は殻のまま。純粋なのに埋まっていた論理だけ抜く。
- **この段は watch の純粋部だけ**: `endOnlineGame`（IO-3）は次段。触るのは reducers.js（移設・追加）と board.js の enterWatchMode／metaToLoadedMeta 呼び出し元のみ。
