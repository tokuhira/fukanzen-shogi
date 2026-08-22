# 不完全将棋 実装指示書 — 記録係三段目 Seal-4（封蝋の検証表示）

> 対象実行者: Claude Code（Sonnet 5）
> 錨: `不完全将棋_記録係三段目_封蝋アーク_概観と段組.md`（同ディレクトリ）。**本書と食い違ったら概観が正**。特に §0.1（封蝋が塞ぐ穴・塞がない穴）・§4 Seal-4・不変の原則 7（検証できない署名に価値はない）・8（「検証不可」と「改竄」を混同しない）。
> 前提: **Seal-1〜3 が着地し、本番デプロイ済み**（`f237231`・`e093fab`・`30ff817`・`fd7e547`・`fae0ba4`・`14cbb07`）。本番の記録係は封蝋を押しており、`GET /recorder/keys` が公開鍵を返す。server テスト 62 件・web テスト 132 件。配布 v0.13.0 / web `?v=` 0.13.0 / RULE 0.7 / PROTOCOL 5。
> 触る現物: **`web/seal-verify.js`（新設）**・**`web/test/seal-verify.test.js`（新設）**・`web/board.js`・`web/online.js`・`web/reducers.js`・`web/test/reducers.test.js`・`web/index.html`。
> 検証済み: **§1 のモジュールと §6 のテストは実際に書いて走らせ、22 件パスを確認済み**（web 全体で 154 件）。うち中核は**本番が 2026-08-22 に実際に綴じた記録**をフィクスチャに使い、`verified` と判定されることを確かめてある。改竄・非対応・未知鍵の各経路も実測。確認後、現物は削除してある（実装者が置き直す）。
> 性格: 封蝋アークの**最終段**。ここまで押されてきた封蝋を、初めて人が目で確かめられるようにする。**このプロジェクト初のクロスオリジン `fetch()`** を web に導入する段でもある。

---

## 0. 目的と範囲

- **作るもの**:
  1. `web/seal-verify.js` — 封蝋の判定（純粋・§1）
  2. `web/test/seal-verify.test.js` — 本番フィクスチャによるテスト（§6）
  3. 殻の配線 — fetch と表示（`board.js`・`online.js`・`index.html`・§3）
  4. 状態のリセット経路への組み込み（`reducers.js`・§4）
  5. web `?v=` の前進（§7）
- **やらないもの（実装者の担当外）**:
  - **デプロイ**（`npx wrangler pages deploy`）。**実装者は触らない。** コードとテストが着地したあと、別途人が実行する（§8）。
- **作らないもの（絶対に触らない）**:
  - `server/` 一切（Seal-1〜3 で確定済み・本番稼働中）
  - `engine/`・`protocol/`・`tui/`・wasm の再ビルド
  - 記録係の招待フロー（`confirm()` 依存の入口）——**根治は別アーク**（バックログ §C の UI アーク）。本段は表示を足すだけ。
  - `archivedLink`・`recordStatusText` の既存の意味（封蝋の表示は**別の行**として足す）

---

## 1. `web/seal-verify.js`（新設・**このまま写す**）

**実測検証済みのコードである。書き換えないこと。** 判定順・失敗の分け方それぞれに理由がある（§2）。

```js
// 封蝋の検証（記録係三段目 Seal-4）。純粋——DOM・可変状態・fetch に非依存。
// crypto は既定で globalThis.crypto.subtle（ブラウザと node の双方に在る）を使い、
// テストからは差し替えられる（unsupported 経路を試すため）。

export const SEAL_MAGIC = 'fukanzen-shogi-seal';
export const SEAL_FORMAT_VERSION = 1;

export function fromBase64Url(s) {
  const b64 = s.replace(/-/g, '+').replace(/_/g, '/');
  const pad = b64.length % 4 === 0 ? '' : '='.repeat(4 - (b64.length % 4));
  const bin = atob(b64 + pad);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

async function sha256Hex(text, subtle) {
  const d = await subtle.digest('SHA-256', new TextEncoder().encode(text));
  return Array.from(new Uint8Array(d)).map(b => b.toString(16).padStart(2, '0')).join('');
}

// 行ベース statement を { version, kind, id, ... } へ。壊れていれば null。
export function parseStatement(statement) {
  if (typeof statement !== 'string') return null;
  const lines = statement.split('\n');
  const head = lines[0]?.split(' ');
  if (!head || head[0] !== SEAL_MAGIC) return null;
  const version = Number(head[1]);
  if (!Number.isInteger(version)) return null;
  const fields = { version };
  for (const line of lines.slice(1)) {
    const i = line.indexOf(' ');
    if (i < 0) return null;
    fields[line.slice(0, i)] = line.slice(i + 1);
  }
  return fields;
}

export async function sealVerdict(id, envelope, keys, subtle = globalThis.crypto?.subtle) {
  const seal = envelope?.seal;
  if (!seal) return 'unsealed';
  if (!subtle) return 'unsupported';
  if (seal.alg !== 'Ed25519') return 'unsupported';

  const st = parseStatement(seal.statement);
  if (!st) return 'tampered';
  if (st.version !== SEAL_FORMAT_VERSION) return 'unsupported';  // 未来の書式は読めない

  const key = (keys || []).find(k => k && k.kid === seal.kid);
  if (!key) return 'unknown_key';
  if (st.kid !== seal.kid) return 'tampered';
  if (st.id !== id) return 'tampered';
  if (st.archived_at !== envelope.archived_at) return 'tampered';

  if (envelope.finalized === true) {
    if (st.kind !== 'finalized') return 'tampered';
    if (st.witnesses !== String(envelope.witnesses)) return 'tampered';
    if (st.id !== await sha256Hex(envelope.text, subtle)) return 'tampered';
  } else if (envelope.disputed === true) {
    if (st.kind !== 'disputed') return 'tampered';
    const [a, b] = envelope.texts || [];
    if (st.text_a !== await sha256Hex(a ?? '', subtle)) return 'tampered';
    if (st.text_b !== await sha256Hex(b ?? '', subtle)) return 'tampered';
  } else {
    return 'tampered';
  }

  // 三つの失敗を混同しないため、デコードと importKey を別々に受ける。
  // 鍵材が壊れている（公開口の問題）／環境が Ed25519 を扱えない／
  // 署名が壊れている（記録の問題）は、それぞれ別の知らせ。
  let rawPub;
  try { rawPub = fromBase64Url(key.public_key); } catch { return 'unknown_key'; }
  let sigBytes;
  try { sigBytes = fromBase64Url(seal.sig); } catch { return 'tampered'; }

  let pub;
  try {
    pub = await subtle.importKey('raw', rawPub, { name: 'Ed25519' }, false, ['verify']);
  } catch {
    return 'unsupported';   // この環境は Ed25519 を扱えない
  }
  try {
    const ok = await subtle.verify({ name: 'Ed25519' }, pub, sigBytes,
      new TextEncoder().encode(seal.statement));
    return ok ? 'verified' : 'tampered';
  } catch {
    return 'tampered';
  }
}
```

### 1.1 なぜこの形か（変えてはならない理由）

- **`subtle` は既定引数**（`globalThis.crypto?.subtle`）。ブラウザにも node にもあるので、注入の儀式なしに**本番と同じ経路をテストが通る**。テストが `unsupported` を試すときだけ差し替える。
  - **注意**: 既定引数は `undefined` で発動する。「`subtle` が無い環境」を試すテストは **`null` を渡す**こと（`undefined` だと既定値が効いて実環境の crypto が使われ、テストが素通りする——実際に一度踏んだ）。
- **`id` を引数で受ける**。`statement.id` が **KV キーと一致するか**まで見るため。envelope だけでは disputed の `id`（ランダム UUID）を検証できない。
- **未来の封蝋書式は `unsupported`**（`tampered` ではない）。`fukanzen-shogi-seal 2` が来たとき、古いクライアントが「改竄だ」と叫ぶのは嘘になる。**読めないことと、壊れていることは違う**（概観 不変の原則 8）。
- **鍵材の破損は `unknown_key`**（`unsupported` ではない）。公開口が壊れたのか、環境が非対応なのかは別の問題。**この分離は、プロトタイプ検証中にフィクスチャの公開鍵へ全角混入の事故が起きて発見した**——当時は「この環境では検証できません」と表示され、原因究明が遠回りになった。
- **disputed は `text_a`/`text_b` を `texts[0]`/`texts[1]` と突き合わせる**。Seal-2 の呼び出し側（`_handleTestimony` が `verdict.idA`/`idB` を渡す 1 行）はテストで守られていないので、順序違いをここで構造的に捕まえる（`fd7e547` のレビューで判明した申し送り）。

---

## 2. 五つの判定と、表示する言葉

**Sonnet に文言を考えさせない。以下をそのまま使う。**

| 判定 | 意味 | 表示 |
|---|---|---|
| `verified` | 封蝋も中身も無傷 | `封蝋を確認しました（綴じられた時のままです）` |
| `unsealed` | 封蝋が無い＝**導入前の記録**。失敗ではない | `封蝋なし（封蝋の導入前に綴じられた記録です）` |
| `tampered` | 封蝋と中身が食い違う | `⚠️ 封蝋と中身が一致しません` |
| `unknown_key` | 記録係の公開鍵に無い `kid` | `未知の鍵で封じられています（検証できません）` |
| `unsupported` | 環境が Ed25519 非対応／未知の alg・書式版 | `この環境では封蝋を検証できません` |

さらに**殻の側**（判定に至れなかった場合）:

| 状況 | 表示 |
|---|---|
| 検証中 | `封蝋を検証中…` |
| `/archive/:id` が 404 | `検証できません（記録を取得できませんでした）` |
| fetch が失敗（通信断など） | `検証できません（通信に失敗しました）` |

> **`unsealed` を失敗として書かないこと。** 本番には封蝋導入前の記録が **25 件**あり（2026-08-22 時点）、それらは正しく `unsealed` になる。「封蝋なし」は事実の報告であって、警告ではない。
> **`unsupported` と `tampered` の文言を近づけないこと**（概観 不変の原則 8）。前者は環境の話、後者は記録への疑い——読む人にとって全く違う知らせ。

---

## 3. 殻の配線

### 3.1 `web/online.js` — 公開鍵の URL

`archiveUrl(id)` の隣に足す（同じ `WS_BASE_URL` から導く）:

```js
/** 記録係の公開鍵の取り出し URL（GET /recorder/keys・記録係三段目 Seal-3）。 */
export function recorderKeysUrl() {
  const httpBase = WS_BASE_URL.replace(/^wss:/, 'https:').replace(/^ws:/, 'http:');
  return `${httpBase}/recorder/keys`;
}
```

### 3.2 `web/board.js` — fetch と判定（**このプロジェクト初のクロスオリジン fetch**）

state に一つ足す（`archivedLink` の隣）:

```js
  sealStatusText: '',              // 封蝋の検証結果表示（記録係三段目 Seal-4）
```

判定を取りに行く関数を足す（`buildArchiveText` の近くなど、素直な場所へ）:

```js
// 綴じられた記録を取りに行き、封蝋を検証して表示へ流す（記録係三段目 Seal-4）。
// このプロジェクト初のクロスオリジン fetch——サーバ側の CORS は Seal-3 で開けてある。
// 独自ヘッダを付けないこと（付けると preflight が要る。Seal-3 は OPTIONS を持たない）。
async function verifyArchivedSeal(id) {
  if (!id) return;
  update({ sealStatusText: '封蝋を検証中…' });
  try {
    const [archRes, keysRes] = await Promise.all([
      fetch(archiveUrl(id)),
      fetch(recorderKeysUrl()),
    ]);
    if (!archRes.ok) {
      update({ sealStatusText: '検証できません（記録を取得できませんでした）' });
      return;
    }
    const envelope = await archRes.json();
    const keys = keysRes.ok ? ((await keysRes.json()).keys ?? []) : [];
    update({ sealStatusText: SEAL_TEXT[await sealVerdict(id, envelope, keys)] ?? '' });
  } catch {
    update({ sealStatusText: '検証できません（通信に失敗しました）' });
  }
}
```

`SEAL_TEXT` は §2 の表をそのまま定数に（モジュール冒頭へ）:

```js
const SEAL_TEXT = {
  verified:    '封蝋を確認しました（綴じられた時のままです）',
  unsealed:    '封蝋なし（封蝋の導入前に綴じられた記録です）',
  tampered:    '⚠️ 封蝋と中身が一致しません',
  unknown_key: '未知の鍵で封じられています（検証できません）',
  unsupported: 'この環境では封蝋を検証できません',
};
```

import を足す: `import { sealVerdict } from './seal-verify.js';` と、`online.js` から `recorderKeysUrl`。

**呼ぶ場所は 4 つ**——`archivedLink` を非 null の id で設定しているところ**すべて**:

1. 観戦側の `onArchived(id)`
2. 観戦側の `onRecordDisagreement(idA, idB, id)`
3. プレイヤー側の `onArchived(id)`
4. プレイヤー側の `onRecordDisagreement(idA, idB, id)`

いずれも既存の `update(...)` の**直後**に `verifyArchivedSeal(id);` を置く（`await` しない——表示は後から追いつけばよい。既存の `disconnectOnline()` 等の順序を変えないこと）。`onRecordDisagreement` の `id` は null がありうるので、`verifyArchivedSeal` 冒頭のガードに任せる。

### 3.3 `web/index.html` — 表示先

`record-info-row` に兄弟を一つ足す:

```html
    <div id="record-info-row">
      <span id="record-info-text"></span>
      <span id="seal-info-text"></span>
      <button id="btn-copy-record-link" hidden>記録リンクをコピー</button>
    </div>
```

> **盤しか無い UI に一行足すだけの、最小の置き場**である。恒常的な UI 面（招待トグル・席の状態・書庫）はバックログ §C の UI アークで作る。**ここで作り込まないこと。**

### 3.4 `render()` — 表示

記録係の状態表示のすぐ後ろへ:

```js
  const sealText = document.getElementById('seal-info-text');
  if (sealText) sealText.textContent = state.sealStatusText;
```

---

## 4. 状態のリセット経路（**見落としやすい・テストが落ちて気づく**）

`sealStatusText` は**新しい対局に持ち越してはならない**。`recordStatusText` と `archivedLink` を初期化しているところ**すべて**に足す:

- `web/reducers.js` の **`resetOnlineReduce()`** — `sealStatusText: ''` を追加
- `board.js` の `enterWatchMode` 冒頭の `update({ watchMode: true, ... })`
- `board.js` の観戦初期化で `recordStatusText: '', archivedLink: null` を並べているところ
- `board.js` の `leaveWatchMode` の `update({ watchMode: false, ... })`

> **`web/test/reducers.test.js` が落ちる。** 「11 のオンライン関連キーをすべて初期値へ戻す」というテストが `toEqual` で**キー集合を丸ごと固定している**ため、キーを足すと必ず失敗する。テスト側にも `sealStatusText: ''` を足し、説明文の件数も合わせること。**これは正しい失敗**——リセット漏れを防ぐために置かれた歯であり、黙らせるのではなく追従させる。

---

## 5. 触らない境界（確認用チェックリスト）

- `server/`・`engine/`・`protocol/`・`tui/` の差分が**ゼロ**。
- `web/wasm/`・`web/notation-wasm/`・`web/protocol-wasm/` の差分が**ゼロ**（wasm は再ビルドしない）。
- 記録係の招待フロー（`confirm()`・`sendRecordAccept`/`Decline`）の差分が**ゼロ**。
- `archivedLink`・`recordStatusText` の**意味と文言**が無変更（封蝋の表示は別フィールド・別要素）。
- `board.js` の既存の非同期順序（`disconnectOnline()`・`_pendingRecordDisconnect` の扱い）が無変更。

---

## 6. テスト（`web/test/seal-verify.test.js` 新設）

### 6.1 フィクスチャは**本番の実物**（ネットワークには触らない）

2026-08-22 に本番の記録係が実際に綴じ、封じた記録。**この値をそのまま使う**（実測で `verified` になることを確認済み）。

```js
const REAL_ID = "c42e4c136d3eabb84543026568a67a59520ccec1107a595520266c95ed20a880";
const REAL_ENVELOPE = {
  finalized: true,
  text: "fukanzen-shogi-archive 1\nrule 0.7\nprotocol 5\napp 0.13.0\nsente -\ngote -\nresult resign draw\nsfen lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
  archived_at: "2026-08-22T04:30:32.536Z",
  witnesses: 2,
  seal: {
    alg: "Ed25519",
    kid: "2f05fdcbf64e2008",
    statement: "fukanzen-shogi-seal 1\nkind finalized\nid c42e4c136d3eabb84543026568a67a59520ccec1107a595520266c95ed20a880\nwitnesses 2\narchived_at 2026-08-22T04:30:32.536Z\nkid 2f05fdcbf64e2008",
    sig: "hBbLvP-F2BQZoCvjeU2XpWglrJ64QKfxe9pP1dH3-5-XvaFlFju-F13lhtdMjR_ZryhUQQ3tryRq9wXdr65rAA",
  },
};
const REAL_KEYS = [{
  kid: "2f05fdcbf64e2008", alg: "Ed25519",
  public_key: "ZgggVUj4KlZZYpYxDmykQuvU7ytr1UmvvQVvHf5t_dE",
}];
```

> **`public_key` と `sig` は一文字も違えてはならない。** 検証中、`public_key` に全角文字が一文字紛れて `unsupported` になり、原因の切り分けに手間取った事故が実際に起きている。コピー後、`REAL_ENVELOPE` が `verified` になることを最初に確かめること。

**disputed は本番に実例が無い**（証言の不一致が起きていない）ので、テスト内でその場で鍵を作って封じる。node の `globalThis.crypto.subtle` は Ed25519 の `generateKey`/`sign`/`exportKey("raw")` をすべて備えている（実測確認済み）。

### 6.2 書くテスト（22 件・実測でこの構成が通っている）

**本番の実物**: `verified` になる。

**改竄の検知**（すべて `tampered`）: 本文を 1 文字変える／平文 `witnesses` を書き換える／平文 `archived_at` を書き換える／`sig` を差し替える／statement の `kid` だけ書き換える／別 `id` の記録として渡す／statement が壊れている。

**失敗ではない状態**: 封蝋なし→`unsealed` ／ 鍵が無い→`unknown_key` ／ **公開鍵の材が壊れている→`unknown_key`**（`unsupported` ではない）／ 未知の `alg`→`unsupported` ／ **未来の書式版→`unsupported`**（`tampered` ではない）／ Ed25519 非対応の環境→`unsupported`（`importKey` が throw する stub を渡す）／ **`subtle` が無い→`unsupported`（`null` を渡すこと。`undefined` は既定値が発動する）**。

**disputed**: 正しければ `verified` ／ 証言 A を書き換えると `tampered` ／ **両証言を入れ替えると `tampered`**（順序も封じられている＝§1.1 の申し送りの回収）。

**`parseStatement`**: 正しい statement を field へ分解する／magic 違いで `null`／空白の無い行で `null`／文字列でなければ `null`。

### 6.3 受け入れ基準

- `cd web && npx vitest run` が通り、**既存 132 件が 1 件も落ちない**（`reducers.test.js` は §4 に従って更新した上で通ること）。
- `web/test/seal-verify.test.js` が **22 件前後**通る。
- §5 のチェックリストがすべて成り立つ（`git diff --stat` で確認）。
- **`tampered` のテストが本当に歯を持つこと**を一度確認する: `sealVerdict` の `if (st.id !== await sha256Hex(envelope.text, subtle)) return 'tampered';` を一時的に消し、「本文を1文字変えると tampered」が落ちることを見てから戻す。（Seal-1・Seal-2 で二度、歯の無いテストが見つかっている。読んで納得せず、壊して確かめること。）
- **ここまでで実装者の仕事は完了**。§8 のデプロイへは進まない。コミットして手を止めること。

---

## 7. 版の刻み

- **web `?v=` を 0.13.0 → 0.13.1 へ**。`index.html` 内の `?v=` を**すべて**更新する（JS も CSS も）。web の JS・HTML を変えたら必ず前進させる。
- **配布版（TUI）据え置き**（v0.13.0）。`web/` と `docs/` しか触らないので、タグは打たない。
- **RULE 0.7・PROTOCOL 5・アーカイブ書式版 1・封蝋書式版 1 すべて据え置き。** 検証は読む側の話で、綴じる側の書式には触れない。
- **wasm を再ビルドしない。** engine/notation/protocol のいずれも変更しないので、`web/wasm/` 等はそのまま。

---

## 8. デプロイ（実装者の担当外）

> **実行者: 人（Opus と一緒に）。実装者（Sonnet）はここを実行しない。**

```bash
npx wrangler pages deploy
```

`git push` だけでは本番へ反映されない。デプロイ後の確認:

1. `https://fukanzen-shogi.pages.dev` を開き、記録係に招いた対局を一局終わらせる
2. 終局後に **`封蝋を確認しました（綴じられた時のままです）`** が出ること
3. 封蝋導入前の記録（例: `17cfd243de599b1c3e065b52d0af343e852c9227049530c8fd396afbcbd059a2`）を読み込んだときに `封蝋なし` と出ること（`unsealed` 経路の実地確認）
4. ブラウザの devtools で `/archive/:id` と `/recorder/keys` への fetch が **CORS で弾かれていない**こと

---

## 9. 次への申し送り

- **アークの総括**を書く（`docs/archive/recorder-seal_総括_Seal1からSeal4.md`）。Seal-1〜4 の道のり、実測で見つかった罠（`archived_at` の二重生成・CORS・node と workerd の JWK 不一致・歯の無いテスト二件）、封蝋が塞ぐ穴と塞がない穴（概観 §0.1）を残す。バックログの §A は「済」へ落とし、開いている次の畝だけを残す。
- **§0.1 の残る穴**は開いたまま: 削除の検知（書庫の一覧が入れば「あったはずのものが無い」に気づける）と、鍵を持つ運営者による捏造（複数記録係の領分）。総括の非ゴールへ。
- **UI アーク**（バックログ §C）が次の候補。本段で足した `seal-info-text` は**盤の脇の一行**にすぎず、招待トグル・席の状態・書庫の導線を置く面はまだ無い。本段を実装すると、その狭さが実感として分かるはず。
- **AI 参画**（バックログ §E）——器（封蝋）が整ったので、水（AI が量産する棋譜）を流せる。

---

## 10. 不変の原則（本段が守るもの）

1. **「検証不可」と「改竄」を混同しない**（概観 不変の原則 8）。環境の制約・未来の書式・未知の鍵・鍵材の破損は、いずれも記録への疑いではない。文言も分ける。
2. **`unsealed` は失敗ではない**。封蝋導入前の記録が本番に 25 件ある。事実として報告する。
3. **検証はサーバの言葉を信じない**。`archived` メッセージに封蝋を相乗りさせず、**保存された現物を取りに行く**（概観 §4.1）。第三者が後日辿るのと同じ経路を、web も通る。
4. **判定は純粋、fetch は殻**。`sealVerdict` は DOM も fetch も知らない。既存の `reducers.js`・`view-model.js` と同じ作法。
5. **独自ヘッダを付けない**。付けた瞬間 preflight が要り、Seal-3 の「`OPTIONS` を作らない」判断が崩れる。
6. **盤の脇に一行だけ**。恒常的な UI 面は別アークの仕事。ここで作り込まない。
