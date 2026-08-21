# 不完全将棋 実装指示書 — 記録係三段目 Seal-2（封蝋を押す）

> 対象実行者: Claude Code（Sonnet 5）
> 錨: `不完全将棋_記録係三段目_封蝋アーク_概観と段組.md`（同ディレクトリ）。**本書と食い違ったら概観が正**。特に §1（何を封じ何を封じないか）・§2.1（一方向合成）・§3（degradation）・不変の原則 3（署名は二証人の上に乗る層で、土台ではない）。
> 前提: **Seal-1 が着地済み**（commit `f237231`・テスト強化 `e093fab`）。`server/src/logic.ts` に `sealFinalized` / `sealDisputed` / `loadRecorderKey` / `verifySeal` / `toBase64Url` / `fromBase64Url` が入っている。配布 v0.13.0 / RULE 0.7 / PROTOCOL 5 / アーカイブ書式版 1。
> 触る現物: **`server/src/room.ts`** と、**新設する `server/test/room-seal.test.ts`**。
> 検証済み: 本書 §3・§4 のコードと §6 のテスト骨格は、実際に room.ts へ当てて走らせ、**7 件パス**を確認済み（typecheck も通る）。さらに §1 の罠を再現（`loadRecorderKey` を try の外へ移動）すると **テスト c・e が落ちる**ことも確認した——テストに歯があることの実測。確認後、現物は復元してある。
> 性格: 封蝋アークの**本体**。Seal-1 で作った核を綴じの経路へ配線する。ここで初めて封蝋が実際に押される。ただし**まだ誰も検証できない**（公開鍵の口は Seal-3）。

---

## 0. 目的と範囲

- **作るもの**:
  1. `Env` に `RECORDER_SIGNING_KEY?: string`（§2）
  2. `_archiveFinalized` が封蝋を押してから put する（§3）
  3. `_archiveDisputed` が封蝋を押してから put する＋呼び出し側がハッシュを流す（§4）
  4. `server/test/room-seal.test.ts` 新設（§6）
- **作らないもの（絶対に触らない）**:
  - `server/src/logic.ts`（Seal-1 で確定済み。**一行も変えない**）
  - `server/src/index.ts`（公開鍵の口・CORS は Seal-3）
  - `web/` 一切（検証表示は Seal-4）
  - `_archiveFragment`（**封蝋の対象外**・概観 §7。content-address されていないので封じる意味が薄い）
  - **綴じの判断そのもの**——招待ゲート（`recording`）・「綴じてから拭く」・`shouldArchiveFragment`・`evaluateTestimonies` の評決・`archived` フラグの立て方・`_broadcastArchived` / `_broadcastRecordDisagreement` の呼び方。封蝋は put の直前に一層かぶせるだけ。

---

## 1. ⚠️ 最重要 — 封蝋の失敗が記録の喪失に化けてはならない

**本段で唯一、間違えると実害が出る箇所。** 先に読むこと。

`loadRecorderKey` は Secret が**壊れているとき throw する**（Seal-1 §5 の仕様。設定ミスを黙って握り潰さないため）。この throw をどこで受けるかで、記録が残るか消えるかが決まる。

現状の綴じ経路は二つとも、`_archiveFinalized` の例外を「ログするだけ」で受けている:

**経路A — 二証人一致（`_handleTestimony`・room.ts:330-344）**
```ts
try {
  if (verdict.kind === "matched") {
    await this._archiveFinalized(a.text, verdict.witnesses);
    await this.state.storage.put("archived", true);
    this._broadcastArchived(verdict.id);
  } else { ... }
} catch (err) {
  this.log(`testimony archive failed: ${String(err)}`);   // ← 綴じ直さない
}
```

**経路B — 単独証言（`webSocketClose`・room.ts:235-245）**
```ts
if (recording && !archived && this._testimonies.size === 1 && solo && isValidTestimonyText(solo.text)) {
  try {
    const id = await this._archiveFinalized(solo.text, 1);
    ...
  } catch (err) {
    this.log(`single-witness archive failed: ${String(err)}`);   // ← 綴じ直さない
  }
} else {
  await this._archiveCurrentIfNeeded();     // ← if 側に入ったので、ここへは来ない
}
this._testimonies.clear();
void this.state.storage.delete("gameStarted");   // ← そして拭かれる
```

もし `_archiveFinalized` の中で **KV put より前に** `loadRecorderKey` が throw すると:

- 経路A: 確定局が書かれず `archived` も立たない。後で断片（ランダム UUID・witnesses なし）に落ちる可能性が残るだけ。
- 経路B: **記録は完全に失われる。** catch に落ちるので断片綴じ（`else` 側）にも入らず、直後に `gameStarted` が消される。

つまり **Secret の設定ミス一つで、確定局が消える**。これは不変の原則 3（署名は二証人の上に乗る層で、土台ではない）の正面からの破れ。

### 対策（構造で守る）

**鍵の読み込みごと、封蝋の try に入れる。put は try の外に置き、必ず到達させる。**

```
envelope を組む（封蝋なし）
        ↓
try { 鍵を読む → 封蝋を押す }  catch { ログだけ・envelope は封蝋なしのまま }
        ↓
KV へ put ← ここへは何があっても到達する
```

`await loadRecorderKey(this.env)` を try の**外**に書いてはならない。§3・§4 のコードをそのまま写せば正しくなる。

---

## 2. Env と import

```ts
interface Env {
  SPECTATE_TOKENS: KVNamespace;
  ARCHIVES: KVNamespace;
  RECORDER_SIGNING_KEY?: string;   // ← 追加（optional。未設定が正常な状態）
}
```

`./logic` からの import に 3 つ足す（既存の並びを崩さず末尾側へ）:

```ts
  sealFinalized,
  sealDisputed,
  loadRecorderKey,
```

---

## 3. `_archiveFinalized`（room.ts:371）

`id` は既にこの関数の中で計算されている（`sha256Hex(text)`）ので、引数を増やす必要はない。**シグネチャは変えない。**

```ts
  private async _archiveFinalized(text: string, witnesses: number): Promise<string> {
    const id = await sha256Hex(text);
    let envelope = buildFinalizedEnvelope(text, witnesses);
    // 封蝋（記録係三段目 Seal-2）。鍵の読み込みごと try に入れる——壊れた Secret で
    // loadRecorderKey が throw しても、下の put へ必ず到達させるため（本書 §1）。
    // 封蝋の失敗が記録の喪失に化けてはならない（概観 不変の原則 3）。
    try {
      envelope = await sealFinalized(envelope, id, await loadRecorderKey(this.env));
    } catch (err) {
      this.log(`seal failed (archiving unsealed) id=${id}: ${String(err)}`);
    }
    await this.env.ARCHIVES.put(id, JSON.stringify(envelope));
    this.log(`archived finalized id=${id} witnesses=${witnesses} sealed=${!!envelope.seal}`);
    return id;
  }
```

- `const envelope` → `let envelope` に変わる点に注意。
- ログに `sealed=` を足すのは、本番で封蝋が押されているかを `wrangler tail` で確かめるため（Seal-3 のデプロイ後に使う）。

---

## 4. `_archiveDisputed`（room.ts:381）と呼び出し側

封蝋には両証言の SHA-256 が要る。**`evaluateTestimonies` が既に計算している**（`verdict.idA` / `verdict.idB`）のに、現状の呼び出しはそれを捨てている。再ハッシュせず流す（概観 §4）。

```ts
  private async _archiveDisputed(
    texts: [string, string],
    hashes: { a: string; b: string },
  ): Promise<string> {
    const id = crypto.randomUUID();
    let envelope = buildDisputedEnvelope(texts);
    // 封じるのは「両言い分をこう受け取った」という受領の事実であって、
    // どちらが正しいかではない（審判なし・記録係二段目 §14-2）。
    try {
      envelope = await sealDisputed(envelope, id, hashes, await loadRecorderKey(this.env));
    } catch (err) {
      this.log(`seal failed (archiving unsealed) id=${id}: ${String(err)}`);
    }
    await this.env.ARCHIVES.put(id, JSON.stringify(envelope));
    this.log(`archived disputed id=${id} sealed=${!!envelope.seal}`);
    return id;
  }
```

呼び出し側（room.ts:338）を 1 行だけ変える:

```ts
        const id = await this._archiveDisputed([a.text, b.text], { a: verdict.idA, b: verdict.idB });
```

**`texts` の順序と `hashes` の順序が対応していること**（`a.text` ↔ `verdict.idA`、`b.text` ↔ `verdict.idB`）。`evaluateTestimonies(a.text, b.text)` の引数順がそのまま `idA`/`idB` なので、上の書き方で正しい。

---

## 5. 触らない境界（確認用チェックリスト）

実装後、以下がすべて成り立つこと:

- `server/src/logic.ts` の差分が**ゼロ**。
- `server/src/index.ts`・`web/` の差分が**ゼロ**。
- `_archiveFragment` の差分が**ゼロ**（封蝋を押さない）。
- `_archiveCurrentIfNeeded`・`shouldArchiveFragment` 周りの差分が**ゼロ**。
- `_handleTestimony` の差分は **§4 の 1 行のみ**（`_archiveDisputed` の呼び出し）。評決・`archived` の put・broadcast には触らない。
- `webSocketClose`（経路B）の差分が**ゼロ**——`_archiveFinalized` の中で完結するので、呼び出し側は変えなくてよい。

---

## 6. テスト（`server/test/room-seal.test.ts` 新設）

### 6.1 前提知識（この段で初めて使う道具）

- **これはこのプロジェクト初の DO テスト**。既存の `logic.test.ts` は純粋ロジックだけで、`room.ts` は一度もテストされていない。踏襲できる既存パターンが無いので、以下の骨格をそのまま使うこと。
- `@cloudflare/vitest-pool-workers` の **`runInDurableObject`** で DO の内部メソッドを直接呼べる（実測確認済み）。WebSocket を張って対局を進める必要はない。
- **Secret は `env` を実行時に書き換えて注入する**（実測確認済み）。`wrangler.toml` に `[vars]` を足してはならない——本番設定にテスト鍵が混ざる。
- **`tsconfig.json` の `include` は `["src"]`** なので、**テストファイルは型検査の対象外**。`env as any` のキャストで構わないし、`npm run typecheck` を通すために型を作り込む必要はない（悩まないこと）。
- **テストごとに違う部屋名を使う**こと（DO の状態が混ざらないように）。

### 6.2 骨格

```ts
import { describe, it, expect } from "vitest";
import { env, runInDurableObject } from "cloudflare:test";
import { verifySeal, fromBase64Url } from "../src/logic";

// ⚠️ TEST 専用の鍵（Seal-1 実装指示書 §8.1 と同一）。本番の
// RECORDER_SIGNING_KEY には絶対に使わない。
const TEST_JWK_OBJ = {
  kty: "OKP", key_ops: ["sign"], alg: "EdDSA", ext: true, crv: "Ed25519",
  x: "s2qbZpbIC74LjfMin-RcE6Fqe99evbEkvbGil7LBSBE",
  d: "yBaW4fmzx56XvAxo17HpRyrXlLt8RwEk7pfSZO0CGCg",
};
const TEST_JWK = JSON.stringify(TEST_JWK_OBJ);
const TEST_KID = "1797cb4996f48595";
const RAW_PUB = fromBase64Url(TEST_JWK_OBJ.x);

const SAMPLE_TEXT = [
  "fukanzen-shogi-archive 1",
  "rule 0.7",
  "protocol 5",
  "app -",
  "sente -",
  "gote -",
  "result resign gote_wins",
  "sfen lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
].join("\n");

// env は共有オブジェクト。各テストで明示的に設定/削除する。
function setSecret(v: string | null): void {
  if (v === null) delete (env as any).RECORDER_SIGNING_KEY;
  else (env as any).RECORDER_SIGNING_KEY = v;
}
function room(name: string): any {
  return (env as any).ROOM.get((env as any).ROOM.idFromName(name));
}
async function readArchive(id: string): Promise<any> {
  const raw = await (env as any).ARCHIVES.get(id);
  return raw ? JSON.parse(raw) : null;
}
```

### 6.3 書くテスト

**a. 鍵ありで finalized に封蝋が乗り、検証が通る**
`setSecret(TEST_JWK)` → `_archiveFinalized(SAMPLE_TEXT, 2)` → 綴じられた envelope の `seal.alg === "Ed25519"`・`seal.kid === TEST_KID`・`verifySeal(RAW_PUB, seal) === true`。

**b. 鍵なしで従来どおりの envelope（degradation）**
`setSecret(null)` → 綴じられ、`seal` が `undefined`。`finalized`/`text`/`archived_at`/`witnesses` は従来どおり揃っている。

**c. ⚠️ 壊れた Secret でも綴じは成立する（本段の本命・§1 の回帰固定）**
```ts
it("壊れた Secret でも綴じは成立する（封蝋の失敗が記録の喪失に化けない）", async () => {
  setSecret("not json{");
  const id = await runInDurableObject(room("seal-broken"), async (i: any) =>
    await i._archiveFinalized("本文", 2));
  const stored = await readArchive(id);
  expect(stored).not.toBeNull();          // ← 綴じられている
  expect(stored.text).toBe("本文");
  expect(stored.witnesses).toBe(2);
  expect(stored.seal).toBeUndefined();    // ← 封蝋だけが無い
});
```
**このテストが落ちたら §1 の対策が効いていない。** `loadRecorderKey` を try の外に書いていないか確認すること。

**d. disputed にも封蝋が乗り、両ハッシュが並ぶ**
`_archiveDisputed(["A", "B"], { a: hashA, b: hashB })` を直接呼ぶ。`seal.statement` の行に `text_a <hashA>` と `text_b <hashB>` が**その順で**含まれ、検証が通る。ハッシュは Seal-1 のテストと同じ値を使ってよい（`SHA-256("A")` = `559aead08264d5795d3909718cdd05abd49572e84fe55590eef31a88a08fdffd`、`SHA-256("B")` = `df7e70e5021544f4834bbee64a9e3789febc4be81470df629cad6ddb03320a5c`）。

**e. disputed も壊れた Secret で綴じは成立する**（c と同じ形で `_archiveDisputed` について）。

**f. `witnesses:1` 経路にも封蝋が乗る**
`_archiveFinalized(SAMPLE_TEXT, 1)` → `seal.statement` に `witnesses 1` が含まれる。

**g. statement の `archived_at` が envelope の `archived_at` と一致する**
綴じられた envelope について `seal.statement` の `archived_at` 行が `stored.archived_at` と一致。**概観 §2.1 が DO 経由でも守られていることの確認**（Seal-1 は関数単体で確認済み、ここは配線後の確認）。

**h. 平文フィールドを書き換えると statement と食い違う（二重掲載が効く証拠）**
```ts
it("平文 witnesses を書き換えても statement は witnesses 2 のまま（食い違いが残る）", async () => {
  setSecret(TEST_JWK);
  const id = await runInDurableObject(room("seal-tamper"), async (i: any) =>
    await i._archiveFinalized(SAMPLE_TEXT, 2));
  const stored = await readArchive(id);
  stored.witnesses = 1;                                     // 改竄を模す
  expect(await verifySeal(RAW_PUB, stored.seal)).toBe(true); // 署名自体は有効
  expect(stored.seal.statement.split("\n")).toContain("witnesses 2");
  expect(stored.witnesses).toBe(1);                          // ← 食い違いが残る
});
```
これが Seal-4 の `sealVerdict` が `tampered` を出す根拠になる。

**i. fragment には封蝋を押さない（対象外・概観 §7）**
`_archiveFragment(record)` を直接呼び、`seal` が `undefined`。`record` は `logic.test.ts` の `SpectateRecord` の作り方に倣う。

### 6.4 受け入れ基準

- `cd server && npm run typecheck` が通る。
- `cd server && npm test` が通り、**既存 43 件が 1 件も落ちない**。
- §5 のチェックリストがすべて成り立つ（`git diff --stat` で確認）。
- **テスト c と e が実装のバグを本当に捕まえること**を、一度確認する: `loadRecorderKey(this.env)` を try の外へ出す改変を一時的に加え、c と e が落ちることを見てから元に戻す。落ちなければテストに歯が無い（Seal-1 で同じ罠を踏んでいる——`e093fab` 参照）。

---

## 7. 版の刻み

**すべて据え置き。** RULE 0.7・PROTOCOL 5・アーカイブ書式版 1・配布 v0.13.0・web `?v=` 0.13.0。封蝋はエンベロープ層で、中身（正準本文）にもワイヤ語彙にも触れない。

**デプロイしない。** 本段だけ本番へ出すと、封蝋が押されるのに誰も検証できない中途半端な状態になる。`wrangler deploy` と `wrangler secret put RECORDER_SIGNING_KEY` は **Seal-3 が着地してから、一緒に**行う（概観 §5）。

---

## 8. 次段への申し送り

- **Seal-3**: `GET /recorder/keys` は `loadRecorderKey(env)` で鍵を読み、`toBase64Url(key.rawPublicKey)` を返す。鍵未設定なら `{ "keys": [] }`。**`/archive/:id` と `/recorder/keys` の両方に CORS**（`Access-Control-Allow-Origin: *`）。本番鍵を生成して `wrangler secret put`、その後 `wrangler deploy`。デプロイ後は `wrangler tail` で `sealed=true` を確認できる（本書 §3 のログ）。
- **Seal-4**: `verifySeal` は署名の正しさしか見ない（`seal.alg` も見ていない）。`sealVerdict` 側で `alg !== "Ed25519"` を `unsupported` として弾き、statement と平文フィールドの突き合わせ（本書 §6.3-h の食い違い検知）を担うこと。

---

## 9. 不変の原則（本段が守るもの）

1. **封蝋の失敗が記録の喪失に化けない**（概観 不変の原則 3）。鍵の読み込みごと try に入れ、put を try の外に置く。これが本段で唯一、間違えると実害が出る点。
2. **綴じの判断に触れない**。招待ゲート・「綴じてから拭く」・二証人評決・`archived` の立て方は無変更。封蝋は put の直前の一層。
3. **審判なし**（記録係二段目 §14-2）。disputed の封蝋は受領の事実であって、優劣の宣告ではない。`hashes` は並置するだけ。
4. **再計算しない**。`evaluateTestimonies` が持っているハッシュを流す。同じものを二度計算する経路を作らない（概観 §2.1 と同じ精神）。
5. **fragment は対象外**。content-address されていないものを封じない。
6. **本段では出荷しない**。検証できない署名だけを本番に置かない（概観 不変の原則 7）。
