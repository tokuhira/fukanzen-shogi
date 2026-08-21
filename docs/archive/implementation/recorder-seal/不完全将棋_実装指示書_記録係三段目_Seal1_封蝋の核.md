# 不完全将棋 実装指示書 — 記録係三段目 Seal-1（封蝋の核）

> 対象実行者: Claude Code（Sonnet 5）
> 錨: `不完全将棋_記録係三段目_封蝋アーク_概観と段組.md`（同ディレクトリ）。**本書と食い違ったら概観が正**。特に §1（何を封じ何を封じないか）・**§2.1（封蝋は envelope から一方向に導出する）**・§3（鍵）・不変の原則。
> 前提: 配布 v0.13.0 / RULE 0.7 / PROTOCOL 5 / アーカイブ書式版 1。記録係は二段目（招待と二証人）まで着地。
> 触る現物: **`server/src/logic.ts` と `server/test/logic.test.ts` の 2 ファイルだけ。**
> 性格: 封蝋アークの一段目＝**核**。純粋関数と WebCrypto の薄いラッパを `logic.ts` へ足すだけで、**まだ誰も呼ばない**（DO への配線は Seal-2、公開口は Seal-3）。呼ばれないので挙動は一切変わらない——このアークで最も安全な段であり、書式と test vector をここで固める。

---

## 0. 目的と範囲

- **作るもの**（すべて `server/src/logic.ts` への加算）:
  1. 型 `Seal`・`RecorderKey`・`RecorderKeyEnv`、既存 envelope 型への `seal?: Seal`（§1）
  2. base64url 変換（§2）
  3. `kidFromRawPublicKey`（§3）
  4. 封蝋 statement の組み立て（§4）
  5. `loadRecorderKey`（モジュール memo つき・§5）
  6. **`sealFinalized` / `sealDisputed`**（一方向合成・§6）＝**本段の要**
  7. `verifySeal`（§7）
- **作らないもの（絶対に触らない）**:
  - `server/src/room.ts`（配線は Seal-2）
  - `server/src/index.ts`（公開口・CORS は Seal-3）
  - `web/` 一切（検証表示は Seal-4）
  - 既存関数の**中身**——`routeDecision`・`evaluateTestimonies`・`isValidTestimonyText`・`buildFinalizedEnvelope`・`buildDisputedEnvelope`・`buildFragmentEnvelope`・`shouldArchiveFragment`・`canAppendTurn`・`sha256Hex`・`nowIso` は**一行も変えない**。envelope 型に optional な `seal?` を足すだけ。
  - `FragmentEnvelope` への `seal`（対象外・概観 §7）

> **`buildFinalizedEnvelope` を変えないことが重要**。封蝋は envelope を組んだ**後**に一方向で被せる（概観 §2.1）ので、組み立て側のシグネチャを触る必要がない。

---

## 1. 型

`logic.ts` の envelope 定義の近くに置く。

```ts
export const SEAL_MAGIC = "fukanzen-shogi-seal";
export const SEAL_FORMAT_VERSION = 1;

/** エンベロープに乗る封蝋（淀川 §1.4：中身ではなく取り扱い層）。 */
export interface Seal {
  alg: "Ed25519";
  kid: string;
  /** 署名対象そのもの。検証側に組み直させないため literal で同梱する（概観 §2）。 */
  statement: string;
  /** statement のバイト列に対する Ed25519 署名（base64url・64 バイト）。 */
  sig: string;
}

/** 記録係の署名鍵。rawPublicKey は JWK の x 成分そのもの（§5）。 */
export interface RecorderKey {
  privateKey: CryptoKey;
  rawPublicKey: Uint8Array;
  kid: string;
}

/** loadRecorderKey が要求する env の最小形（room.ts の Env に依存しない）。 */
export interface RecorderKeyEnv {
  RECORDER_SIGNING_KEY?: string;
}
```

既存の 2 つの envelope 型に **optional で**足す（`FragmentEnvelope` には足さない）:

```ts
export interface FinalizedEnvelope {
  finalized: true;
  text: string;
  archived_at: string;
  witnesses: number;
  seal?: Seal;          // ← 追加
}

export interface DisputedEnvelope {
  finalized: false;
  disputed: true;
  texts: [string, string];
  archived_at: string;
  seal?: Seal;          // ← 追加
}
```

---

## 2. base64url

**Workers に組み込みが無い。以下をそのまま書くこと**（発明しない）。`atob`/`btoa` は workerd で使える（確認済み）。

```ts
export function toBase64Url(bytes: Uint8Array): string {
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export function fromBase64Url(s: string): Uint8Array {
  const b64 = s.replace(/-/g, "+").replace(/_/g, "/");
  const pad = b64.length % 4 === 0 ? "" : "=".repeat(4 - (b64.length % 4));
  const bin = atob(b64 + pad);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}
```

> `String.fromCharCode(...bytes)` の**スプレッドは使わない**。今回の入力は 32〜64 バイトなので実害は出ないが、大きい配列でスタックを吹く書き方をコードベースに残さない。

---

## 3. kid

raw 公開鍵（32 バイト）の SHA-256 の**先頭 16 hex**。

```ts
export async function kidFromRawPublicKey(raw: Uint8Array): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", raw);
  return Array.from(new Uint8Array(digest))
    .map(b => b.toString(16).padStart(2, "0"))
    .join("")
    .slice(0, 16);
}
```

---

## 4. 封蝋 statement（行ベース・engine のアーカイブ書式に倣う）

`engine/src/archive.rs::kifu_to_archive` と同じ作法——**行を配列に積んで `\n` で join、末尾改行なし**。

```ts
export type SealSubject =
  | { kind: "finalized"; id: string; witnesses: number }
  | { kind: "disputed"; id: string; text_a: string; text_b: string };

/**
 * 封蝋の署名対象を組み立てる（純粋）。
 *
 * archived_at は「これから綴じる envelope が既に決めた値」を渡すこと。
 * ここで nowIso() を呼んではならない——二重生成が偽の改竄判定を生む（概観 §2.1）。
 */
export function buildSealStatement(subject: SealSubject, archivedAt: string, kid: string): string {
  const parts = [`${SEAL_MAGIC} ${SEAL_FORMAT_VERSION}`, `kind ${subject.kind}`, `id ${subject.id}`];
  if (subject.kind === "finalized") {
    parts.push(`witnesses ${subject.witnesses}`);
  } else {
    parts.push(`text_a ${subject.text_a}`);
    parts.push(`text_b ${subject.text_b}`);
  }
  parts.push(`archived_at ${archivedAt}`);
  parts.push(`kid ${kid}`);
  return parts.join("\n");
}
```

**行順は固定**（magic → kind → id → 種別固有 → archived_at → kid）。並べ替えると既存の封蝋が検証できなくなる。

---

## 5. 鍵の読み込み（モジュール memo）

- Secret **未設定** → `null` を返す（degradation。ローカル `wrangler dev` や Secret 投入前の正常な状態）。
- Secret **あるが壊れている**（JSON でない・JWK でない） → **throw する**。呼び出し側（Seal-2）が catch してログに出し、封蝋なしで綴じる。**設定ミスを黙って握り潰さない**——綴じは守り、異常は見えるように。
- `importKey` は非同期なので **Promise を memo** する（並行綴じで二重 import しない）。**reject したら memo を捨てる**（次回また試み、またログに出る）。

```ts
let _keyMemo: { secret: string; promise: Promise<RecorderKey> } | null = null;

export async function loadRecorderKey(env: RecorderKeyEnv): Promise<RecorderKey | null> {
  const secret = env.RECORDER_SIGNING_KEY;
  if (!secret) return null;                       // 未設定は正常（封蝋を省く）
  if (_keyMemo && _keyMemo.secret === secret) return _keyMemo.promise;

  const promise = (async (): Promise<RecorderKey> => {
    const jwk = JSON.parse(secret) as JsonWebKey;
    const privateKey = await crypto.subtle.importKey("jwk", jwk, { name: "Ed25519" }, false, ["sign"]);
    // Ed25519 の秘密鍵 JWK は公開鍵成分 x を内包する（実測確認済み）ので、
    // 公開鍵用の別 Secret も exportKey も要らない。
    if (!jwk.x) throw new Error("RECORDER_SIGNING_KEY: JWK に x（公開鍵成分）が無い");
    const rawPublicKey = fromBase64Url(jwk.x);
    const kid = await kidFromRawPublicKey(rawPublicKey);
    return { privateKey, rawPublicKey, kid };
  })();

  _keyMemo = { secret, promise };
  promise.catch(() => { if (_keyMemo?.promise === promise) _keyMemo = null; });
  return promise;
}
```

> `crypto.subtle.importKey("jwk", …)` に渡す JWK は `key_ops:["sign"]` を含む形でよい（`exportKey("jwk")` が出す形そのまま）。

---

## 6. 封蝋を押す（一方向合成）＝**本段の要**

概観 §2.1 の構造をコードにする。**`archived_at` は envelope からしか読まない**。`witnesses` も envelope が持っているので envelope から読む。envelope に無い値（`id`・証言ハッシュ）だけを引数で受ける。

```ts
/**
 * 確定局の envelope に封蝋を押す（概観 §2.1 の一方向合成）。
 *
 * archived_at と witnesses は **envelope から読む**——引数で受け取らない。
 * 二度生成する経路を作らないことが、偽の「改竄」判定を構造的に防ぐ。
 *
 * @param id KV キー（= 正準本文の SHA-256）。envelope に無いので引数で受ける。
 * @param key null（Secret 未設定）なら封蝋を押さず envelope をそのまま返す。
 */
export async function sealFinalized(
  envelope: FinalizedEnvelope,
  id: string,
  key: RecorderKey | null,
): Promise<FinalizedEnvelope> {
  if (!key) return envelope;
  const statement = buildSealStatement(
    { kind: "finalized", id, witnesses: envelope.witnesses },
    envelope.archived_at,
    key.kid,
  );
  return { ...envelope, seal: await signStatement(statement, key) };
}

/**
 * 不一致の envelope に封蝋を押す。
 *
 * 封じるのは「両言い分をこう受け取った」という受領の事実であって、
 * どちらが正しいかではない（審判なし・記録係二段目 §14-2）。
 *
 * @param hashes 両証言の SHA-256。evaluateTestimonies が計算済みの値を
 *               流すこと（本文を再ハッシュしない）。
 */
export async function sealDisputed(
  envelope: DisputedEnvelope,
  id: string,
  hashes: { a: string; b: string },
  key: RecorderKey | null,
): Promise<DisputedEnvelope> {
  if (!key) return envelope;
  const statement = buildSealStatement(
    { kind: "disputed", id, text_a: hashes.a, text_b: hashes.b },
    envelope.archived_at,
    key.kid,
  );
  return { ...envelope, seal: await signStatement(statement, key) };
}

async function signStatement(statement: string, key: RecorderKey): Promise<Seal> {
  const sig = await crypto.subtle.sign(
    { name: "Ed25519" },
    key.privateKey,
    new TextEncoder().encode(statement),
  );
  return { alg: "Ed25519", kid: key.kid, statement, sig: toBase64Url(new Uint8Array(sig)) };
}
```

---

## 7. 検証

```ts
/** 封蝋を raw 公開鍵で検証する。例外は false に潰す（検証失敗と同じ扱い）。 */
export async function verifySeal(rawPublicKey: Uint8Array, seal: Seal): Promise<boolean> {
  try {
    const pub = await crypto.subtle.importKey("raw", rawPublicKey, { name: "Ed25519" }, false, ["verify"]);
    return await crypto.subtle.verify(
      { name: "Ed25519" },
      pub,
      fromBase64Url(seal.sig),
      new TextEncoder().encode(seal.statement),
    );
  } catch {
    return false;
  }
}
```

> **`verifySeal` は署名の正しさだけを見る。** 「statement の `id` が本文の SHA-256 と一致するか」「平文フィールドと statement が一致するか」は**上位（Seal-4 の `sealVerdict`）の仕事**。ここで混ぜない。

---

## 8. テストと受け入れ基準

`server/test/logic.test.ts` に追記（既存の describe を壊さない）。日本語の `describe`/`it`・節番号コメントという既存の作法に合わせること。

### 8.1 固定 test vector（**この値をそのまま使う**）

**⚠️ 以下は TEST 専用の鍵。本番の `RECORDER_SIGNING_KEY` には絶対に使わないこと**（本番鍵は Seal-3 で別途生成する）。Ed25519 の署名は決定的なので（実測確認済み）、この鍵・この statement なら署名は毎回同じ値になる。

```ts
const TEST_JWK = {
  kty: "OKP", key_ops: ["sign"], alg: "EdDSA", ext: true, crv: "Ed25519",
  x: "s2qbZpbIC74LjfMin-RcE6Fqe99evbEkvbGil7LBSBE",
  d: "yBaW4fmzx56XvAxo17HpRyrXlLt8RwEk7pfSZO0CGCg",
};
const TEST_KID = "1797cb4996f48595";
```

正準本文のサンプル（アーカイブ書式 v1 相当）:

```ts
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
// SHA-256:
const SAMPLE_ID = "e730294a6fcd51c0f5f9e7bdfb4de3cb91bee56ae4c10c74ab1630d79b0902da";
```

finalized の golden（`archived_at` は `"2026-08-22T12:34:56.789Z"` 固定）:

```
fukanzen-shogi-seal 1
kind finalized
id e730294a6fcd51c0f5f9e7bdfb4de3cb91bee56ae4c10c74ab1630d79b0902da
witnesses 2
archived_at 2026-08-22T12:34:56.789Z
kid 1797cb4996f48595
```

```ts
const GOLDEN_SIG_FINALIZED =
  "AnyomxmOegNdrBmOxL5MNxM_eHWP37lPXvaKRCJzMoo9BWk2H6lNH00AZUna3JOEY3MphGusMYIJLLXU5wXyCg";
```

disputed の golden（`text_a` = SHA-256("A")、`text_b` = SHA-256("B")、`id` は固定 UUID 文字列）:

```
fukanzen-shogi-seal 1
kind disputed
id 018f3e7a-0000-7000-8000-000000000000
text_a 559aead08264d5795d3909718cdd05abd49572e84fe55590eef31a88a08fdffd
text_b df7e70e5021544f4834bbee64a9e3789febc4be81470df629cad6ddb03320a5c
archived_at 2026-08-22T12:34:56.789Z
kid 1797cb4996f48595
```

```ts
const GOLDEN_SIG_DISPUTED =
  "dXfCD-uNKVGJ7gLOYzinOsIopC44g-YZJietqt0Nknoj3SOBXdj1D8f5sEriWlk384NgsxdO8W5RxuFi_lD2Cg";
```

### 8.2 書くテスト

- **base64url**: 往復（`fromBase64Url(toBase64Url(x)) === x`）／パディング長 0・1・2 の各ケース／`+` `/` を含むバイト列が `-` `_` になる。
- **kid**: `TEST_JWK.x` から `TEST_KID` が出る（決定性）。
- **statement（golden）**: finalized・disputed とも §8.1 の**文字列と完全一致**（`toBe` でバイト一致を見る。snapshot でなく literal で書く）。
- **`loadRecorderKey`**: Secret 未設定で `null`／`TEST_JWK` の JSON で `kid === TEST_KID`／**壊れた JSON で throw**／同じ Secret で二度呼ぶと**同じオブジェクトが返る**（memo が効いている）。
- **署名（golden）**: `sealFinalized` / `sealDisputed` が §8.1 の `sig` と一致する。
- **`sealFinalized` が envelope の `archived_at` をそのまま使う**（**概観 §2.1 の回帰固定・本段で最も大事なテスト**）: `buildFinalizedEnvelope` で作った envelope を封じ、`seal.statement` に含まれる `archived_at` 行が `envelope.archived_at` と一致することを確認する。
- **`witnesses` も envelope から読む**: `witnesses: 1` の envelope を封じると statement が `witnesses 1` になる。
- **鍵 null で素通し**: `sealFinalized(env, id, null)` が `seal` を持たない envelope を返す。
- **`verifySeal`**: 正しい封蝋で `true`／statement を 1 文字変えると `false`／`sig` を壊すと `false`／別の鍵の raw 公開鍵で `false`。
- **既存 envelope の非回帰**: `buildFinalizedEnvelope`/`buildDisputedEnvelope`/`buildFragmentEnvelope` の出力に **`seal` が現れない**（封じるまでは付かない）。

### 8.3 受け入れ基準

- `cd server && npm run typecheck` が通る。
- `cd server && npm test` が通り、**既存テストが 1 件も落ちない**。
- `server/src/room.ts`・`server/src/index.ts`・`web/` の差分が**ゼロ**（`git diff --stat` で確認）。
- 既存関数の中身に差分が無い（envelope 型への `seal?` 追加を除く）。

---

## 9. 版の刻み

**すべて据え置き。** RULE 0.7・PROTOCOL 5・アーカイブ書式版 1・配布 v0.13.0・web `?v=` 0.13.0。本段は誰も呼ばないコードの追加で、外から見える挙動は何も変わらない。デプロイも不要（Seal-3 まで着地してから `wrangler deploy`）。

---

## 10. 次段への申し送り

- **Seal-2**: `room.ts` の `_archiveFinalized`/`_archiveDisputed` が、envelope を組んだ後 KV へ put する**直前**に `sealFinalized`/`sealDisputed` を通す。`Env` に `RECORDER_SIGNING_KEY?: string`。disputed は `evaluateTestimonies` の `verdict.idA`/`idB` を `hashes` として流す（再ハッシュしない）。署名の例外は catch → `this.log` → 封蝋なしで綴じる。
- **Seal-3**: `GET /recorder/keys`（`toBase64Url(key.rawPublicKey)` を返す）＋ **`/archive/:id` と `/recorder/keys` の CORS**。本番鍵の生成と `wrangler secret put`。
- **Seal-4**: `verifySeal` は署名しか見ないので、`sealVerdict` が「statement の `id` ＝ 本文の SHA-256 か」「平文フィールドと statement が食い違わないか」を担う。

---

## 11. 不変の原則（本段が守るもの）

1. **封蝋は envelope から一方向に導出する**（概観 §2.1・不変の原則 6）。`archived_at`・`witnesses` は envelope からしか読まない。時刻を二度生成する経路を作らない。
2. **既存の綴じ判断に触れない**。`buildXxxEnvelope`・`evaluateTestimonies`・`shouldArchiveFragment` は一行も変えない。封蝋は後から被せる層。
3. **鍵が無いのは正常、壊れているのは異常**。前者は `null` で静かに degradation、後者は throw して見えるように。どちらの場合も（Seal-2 で）綴じは成立する。
4. **`verifySeal` は署名の正しさだけを見る**。内容の突き合わせは上位の仕事。層を混ぜない。
5. **書式は固定**。行順・magic・版番号を変えると既存の封蝋が読めなくなる。golden テストがそれを守る。
6. **誰も呼ばないコードを足すだけ**。本段で挙動は変わらない。だから安全に書式を固められる。
