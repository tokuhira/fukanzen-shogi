# 不完全将棋 実装指示書 — 記録係三段目 Seal-3（鍵の公開口と CORS）

> 対象実行者: Claude Code（Sonnet 5）
> 錨: `不完全将棋_記録係三段目_封蝋アーク_概観と段組.md`（同ディレクトリ）。**本書と食い違ったら概観が正**。特に §0.1（封蝋が塞ぐ穴・塞がない穴）・§3（鍵）・§4 Seal-3・不変の原則 7（検証できない署名に価値はない）。
> 前提: **Seal-1・Seal-2 が着地済み**（`f237231`・`e093fab`・`30ff817`・`fd7e547`）。`logic.ts` に封蝋の核、`room.ts` が綴じの直前に封蝋を押す。server テスト 53 件。**まだ未デプロイ**。
> 触る現物: **`server/src/index.ts`** と、新設する **`server/test/index-routing.test.ts`**。
> 検証済み: 本書 §3・§4 のコードと §6 のテスト骨格は、実際に `index.ts` へ当てて走らせ **9 件パス**を確認済み（typecheck も通る）。テスト i（封蝋の `kid` と公開鍵の `kid` の一致）も通る。さらに `/archive/:id` から CORS を外すと **e・f が落ちる**ことも確認した——テストに歯があることの実測。§1 の罠（node の JWK を workerd が拒否する）と §7-1 の生成コマンドも実測で確かめてある。確認後、現物は復元してある。
> 性格: **このアーク初のデプロイを伴う段**。ここまで封蝋は押されていても誰も検証できなかった。公開鍵の口と CORS が揃って初めて「第三者が、書庫を運営する者の言葉を信じずに完全性を確かめられる」（概観 §0.1）が成立する。実装は軽いが、**段取り（§7）が本体**。

---

## 0. 目的と範囲

- **作るもの**:
  1. `Env` に `RECORDER_SIGNING_KEY?: string`（§3）
  2. `GET /recorder/keys`（§3）
  3. **CORS** — `/recorder/keys` と**既存の `/archive/:id`** の両方（§2・§4）
  4. `server/test/index-routing.test.ts` 新設（§6）
  5. **本番鍵の生成とデプロイ**（§7）＝この段の本体
- **作らないもの（絶対に触らない）**:
  - `server/src/logic.ts`・`server/src/room.ts`（Seal-1・Seal-2 で確定済み。**一行も変えない**）
  - `web/` 一切（検証表示は Seal-4）
  - `/watch/:token`・`/room/:key` のルーティング挙動
  - `OPTIONS` ハンドラ（**作らない**。理由は §2.2）
  - 鍵ローテーションの実装（配列で器だけ空ける・概観 §7）

---

## 1. ⚠️ 最重要 — 鍵生成の罠（node と workerd で JWK が食い違う）

**本段で唯一、間違えると本番が静かに壊れる箇所。** 先に読むこと。

Ed25519 の秘密鍵 JWK を **node で生成すると `alg: "Ed25519"`** が入る。ところが **workerd は RFC 8037 の `alg: "EdDSA"` を期待**し、`alg: "Ed25519"` を**拒否する**:

```
DataError: JSON Web Key Algorithm parameter "alg" ("Ed25519")
           does not match requested Ed25519 curve.
```

（実測確認済み。node v24 の `webcrypto.subtle.exportKey("jwk", …)` の出力を `loadRecorderKey` に渡して再現。）

これに気づかず `wrangler secret put` すると:

- `loadRecorderKey` が throw する
- Seal-2 は仕様どおり catch して**封蝋なしで綴じ続ける**（記録は失われない——ここは正しく働く）
- `/recorder/keys` は `{ "keys": [] }` を返す
- **つまり「鍵を入れたのに、何も封印されない」状態が、ログを見ない限り気づけないまま続く**

### 対策 — 最小形の JWK を生成する（検証済みコマンド）

`alg`・`key_ops`・`ext` を落とし、**`kty` / `crv` / `x` / `d` の 4 つだけ**にする。この形は workerd がそのまま受理する（`alg: "EdDSA"` へ書き換える・`alg` を削除する形でも通ることを確認したが、**余計なフィールドが無い最小形が最も安全**）。

```bash
node -e '
const { webcrypto } = require("crypto");
(async () => {
  const kp = await webcrypto.subtle.generateKey({ name: "Ed25519" }, true, ["sign", "verify"]);
  const j = await webcrypto.subtle.exportKey("jwk", kp.privateKey);
  console.log(JSON.stringify({ kty: j.kty, crv: j.crv, x: j.x, d: j.d }));
})();
'
```

**実測確認済み**: このコマンドの出力を `loadRecorderKey` に渡すと `kid` が導出され、`sealFinalized` → `verifySeal` が通る。

> **鍵の取り扱い**: これは**秘密鍵**。ファイルへ落とさず、シェル履歴にも残さないため、**直接 `wrangler secret put` へパイプする**（§7-1）。公開鍵と `kid` はデプロイ後に `/recorder/keys` から取れるので、手元に控える必要はない。
>
> **テスト鍵を本番へ流用しないこと。** `logic.test.ts` / `room-seal.test.ts` の `TEST_JWK` は公開リポジトリに平文で入っている。本番鍵は必ず新規生成する。

---

## 2. CORS — なぜ要るか、どこに要るか

### 2.1 なぜ

web は `fukanzen-shogi.pages.dev`、Worker は `fukanzen-shogi-ws.tokuhira.workers.dev` で**別オリジン**。今まで問題にならなかったのは、web が `/archive/:id` を**リンクとして開くだけ**（トップレベル遷移＝CORS 不要）だったから。**Seal-4 が初めてこれを `fetch()` する**（`board.js`/`online.js` とも現状 `fetch` 0 件——このプロジェクト初のクロスオリジン fetch）。

本番実測（2026-08-22）と `SELF.fetch` での再確認:

```
GET /archive/:id  → 200 / Content-Type: application/json
                     Access-Control-Allow-Origin: (無し)   ← これが塞がっていない
```

CORS ヘッダが無いと、`fetch()` は**ステータスに関係なく TypeError で reject する**。ゆえに:

- **200 応答**にヘッダが要る（本体を読むため）
- **404 応答にも要る**（そうしないと Seal-4 が「記録が無い」と「ネットワークが死んだ」を区別できない）

### 2.2 `OPTIONS` ハンドラは作らない

単純 GET・カスタムリクエストヘッダ無しなので **preflight は飛ばない**。`OPTIONS` を実装する必要はない（過ぎたるは及ばざる）。Seal-4 の `fetch` には独自ヘッダを付けないこと——付けた瞬間 preflight が必要になり、この判断が崩れる。

### 2.3 `*` は妥協ではない

書庫も公開鍵も、誰でも `curl` できる公開データとして設計されている（版図「公開情報は対等、綴じ込みだけが特権」）。`Access-Control-Allow-Origin: *` はその設計を**正直に表明するだけ**であり、Pages のプレビュー配備がランダムなサブドメインを取ることにも耐える。オリジンを絞るのは、守るものが無いのに鍵をかけるのと同じで、意味が無い上に壊れやすい。

---

## 3. `GET /recorder/keys`

`Env` に一つ足す（`index.ts` の `Env` は `room.ts` のものとは別定義。**両方に要る**が、`room.ts` は Seal-2 で済んでいる）:

```ts
interface Env {
  ROOM: DurableObjectNamespace;
  SPECTATE_TOKENS: KVNamespace;
  ARCHIVES: KVNamespace;
  RECORDER_SIGNING_KEY?: string;   // ← 追加
}
```

import を足す:

```ts
import { loadRecorderKey, toBase64Url } from "./logic";
```

エンドポイント本体。**`/archive/:id` のブロックの直後、`/watch/:token` の前**に置く（書庫まわりの公開口を隣り合わせにする）:

```ts
    // 記録係の公開鍵（記録係三段目 Seal-3）。封蝋を第三者が検証するための口。
    // DO を介さない。配列で返すのは継ぎ目の予約——鍵ローテーションと、遠い地平の
    // 複数記録係が、この形のまま載る（概観 §3・プリンシパル設計 §5）。中身は今は書かない。
    if (url.pathname === "/recorder/keys" && request.method === "GET") {
      let keys: unknown[] = [];
      try {
        const key = await loadRecorderKey(env);
        if (key) {
          keys = [{ kid: key.kid, alg: "Ed25519", public_key: toBase64Url(key.rawPublicKey) }];
        }
      } catch (err) {
        // 鍵が壊れている＝設定ミス。ログには残すが、応答は「今は封をしていない」で
        // 揃える——このとき Seal-2 も封蝋なしで綴じているので、系として整合する
        // （封蝋が無い記録に、封蝋を検証する鍵を返しても意味がない）。
        console.log(`[recorder/keys] key load failed: ${String(err)}`);
      }
      return new Response(JSON.stringify({ keys }, null, 2), {
        headers: {
          "Content-Type": "application/json",
          "Cache-Control": "public, max-age=300",
          ...PUBLIC_CORS,
        },
      });
    }
```

- **鍵未設定は `{ "keys": [] }` の 200**（404 ではない）。「記録係は居るが、今は封をしていない」を表す。
- **鍵が壊れていても 200 + 空配列**。ログが唯一の声になるので、§7-4 のデプロイ後確認で必ず見ること。
- **`Cache-Control: public, max-age=300`**。公開鍵は不変だが、鍵の**集合**はローテーションで変わりうる。1 年キャッシュすると将来の入れ替えが苦しくなるので 5 分に留める。

---

## 4. CORS の実装

`index.ts` の先頭近く（`Env` の下）に定数を置く:

```ts
// 書庫も公開鍵も誰でも読める公開データ（版図「公開情報は対等、綴じ込みだけが特権」）。
// * はその設計を正直に表明するもので、オリジンを絞る意味は無い（本書 §2.3）。
const PUBLIC_CORS = { "Access-Control-Allow-Origin": "*" };
```

既存の `/archive/:id` ブロックを、**ヘッダを足すだけ**で書き換える（ロジックには触らない）:

```ts
    if (archiveMatch && request.method === "GET") {
      const id = decodeURIComponent(archiveMatch[1]);
      const raw = await env.ARCHIVES.get(id);
      if (!raw) {
        // 404 にも CORS が要る——無いと Seal-4 が「記録が無い」と
        // 「ネットワークが死んだ」を区別できない（本書 §2.1）。
        return new Response("Not found", { status: 404, headers: { ...PUBLIC_CORS } });
      }
      return new Response(raw, {
        headers: { "Content-Type": "application/json", ...PUBLIC_CORS },
      });
    }
```

**`/watch/:token`・`/room/:key` には CORS を付けない。** WebSocket のアップグレードと DO への委譲であり、`fetch()` で読む対象ではない。末尾の総括 404（`match` しなかった場合）も現状のまま。

---

## 5. 触らない境界（確認用チェックリスト）

実装後、以下がすべて成り立つこと:

- `server/src/logic.ts`・`server/src/room.ts` の差分が**ゼロ**。
- `web/` の差分が**ゼロ**。
- `/watch/:token` ブロックの差分が**ゼロ**。
- `/room/:key` のマッチと委譲の差分が**ゼロ**。
- `/archive/:id` の差分は**ヘッダの追加のみ**（KV 取得・404 判定・`decodeURIComponent` のロジックは無変更）。

---

## 6. テスト（`server/test/index-routing.test.ts` 新設）

### 6.1 道具

Worker のルーティングは **`SELF.fetch`** で叩ける（実測確認済み）。DO を経由する `/room/:key` も含めて、Worker 全体を統合テストできる。

```ts
import { describe, it, expect } from "vitest";
import { SELF, env } from "cloudflare:test";
```

Secret は Seal-2 と同じく `env` の実行時書き換えで注入する（`(env as any).RECORDER_SIGNING_KEY = …`）。`wrangler.toml` に `[vars]` を足さないこと。テストファイルは型検査対象外（`tsconfig.json` の `include` が `["src"]`）なので `as any` で構わない。

### 6.2 書くテスト

**a. 鍵ありで 1 件返る** — `keys.length === 1`、`kid` が `TEST_KID`（`1797cb4996f48595`）、`alg === "Ed25519"`、`public_key` が `TEST_JWK.x` と一致。

**b. 鍵なしで空配列** — `setSecret(null)` → `{ keys: [] }`、ステータス **200**（404 ではないことを明示的に確認）。

**c. 壊れた鍵でも 200 + 空配列** — `"not json{"` → `{ keys: [] }`、ステータス 200（500 にならないこと）。

**d. `/recorder/keys` に CORS ヘッダ** — `Access-Control-Allow-Origin === "*"`。

**e. ⚠️ `/archive/:id` の 200 に CORS ヘッダ**（既存エンドポイントの回帰テスト）— KV に置いた記録を取り出し、`Access-Control-Allow-Origin === "*"` かつ `Content-Type: application/json` かつ本体が壊れていないこと。

**f. ⚠️ `/archive/:id` の 404 に CORS ヘッダ** — 存在しない id で 404、かつ `Access-Control-Allow-Origin === "*"`（§2.1 の理由）。

**g. `Cache-Control` が付く** — `/recorder/keys` に `public, max-age=300`。

**h. 既存ルーティングの非回帰** — 未知トークンの `/watch/:token` が 404、まったく未知のパス（`/nope`）が 404。

**i. Seal-2 との突き合わせ（統合）** — 鍵を入れた状態で `_archiveFinalized` を通した記録を `/archive/:id` から取り出し、その `seal.kid` が `/recorder/keys` の `kid` と**一致する**こと。**封蝋を押した鍵と、公開している鍵が同じものであることの確認**——これが崩れると検証は全部落ちる。Seal-2 のテスト（`runInDurableObject`）と本段のテスト（`SELF.fetch`）を繋ぐ唯一の糸なので、必ず書くこと。

### 6.3 受け入れ基準

- `cd server && npm run typecheck` が通る。
- `cd server && npm test` が通り、**既存 53 件が 1 件も落ちない**。
- §5 のチェックリストがすべて成り立つ（`git diff --stat` で確認）。
- **テスト e・f が本当に歯を持つこと**を一度確認する: `PUBLIC_CORS` の展開を `/archive/:id` から一時的に外し、e・f が落ちることを見てから戻す。（Seal-1・Seal-2 で二度、歯の無いテストが見つかっている。読んで納得せず、壊して確かめること。）

---

## 7. デプロイ（このアーク初・**この段の本体**）

コードが着地して `npm test` が通ってから行う。**順序を守ること**——鍵を入れる前にデプロイすると、一時的に「封蝋なしで綴じる」窓が開く（実害は無いが、その間の記録に封蝋が付かない）。

### 7-1. 本番鍵を生成して投入（秘密鍵をディスクに落とさない）

```bash
cd server
node -e '
const { webcrypto } = require("crypto");
(async () => {
  const kp = await webcrypto.subtle.generateKey({ name: "Ed25519" }, true, ["sign", "verify"]);
  const j = await webcrypto.subtle.exportKey("jwk", kp.privateKey);
  console.log(JSON.stringify({ kty: j.kty, crv: j.crv, x: j.x, d: j.d }));
})();
' | npx wrangler secret put RECORDER_SIGNING_KEY
```

パイプすることで、秘密鍵がファイルにもシェル履歴にも残らない。**§1 の最小形であることが要**。

> **wrangler の認証がこの WSL 環境では定期的に失効する。** `wrangler secret put` が認証エラーになったら `npx wrangler login` を先に通すこと。

### 7-2. デプロイ

```bash
cd server && npx wrangler deploy
```

`git push` だけでは本番へ反映されない。

### 7-3. 公開鍵の口を確認

```bash
curl -s https://fukanzen-shogi-ws.tokuhira.workers.dev/recorder/keys
```

`keys` に 1 件あり `kid` が入っていること。**ここが空配列なら §1 の罠を踏んでいる**（`alg` が `"Ed25519"` のまま入った）——`wrangler tail` にキーの読み込み失敗が出ているはずなので確認し、§1 のコマンドで生成し直す。

### 7-4. CORS を確認

```bash
curl -sI https://fukanzen-shogi-ws.tokuhira.workers.dev/recorder/keys | grep -i access-control
curl -sI https://fukanzen-shogi-ws.tokuhira.workers.dev/archive/does-not-exist | grep -i access-control
```

両方に `access-control-allow-origin: *` が出ること（**404 の側も**）。

### 7-5. 実際に封蝋が押されることを確認

`wrangler tail` を開いた状態で、web で記録係に招いた対局を一局終わらせる:

```bash
cd server && npx wrangler tail
```

ログに `archived finalized id=… witnesses=2 sealed=true` が出ること（`sealed=` は Seal-2 で足したログ）。**`sealed=false` なら鍵が効いていない。**

そのうえで、綴じられた id を取り出して封蝋が乗っているか目で見る:

```bash
curl -s https://fukanzen-shogi-ws.tokuhira.workers.dev/archive/<id> | head -c 400
```

`"seal": { "alg": "Ed25519", "kid": …, "statement": …, "sig": … }` が入っていること。

---

## 8. 版の刻み

- **RULE 0.7・PROTOCOL 5・アーカイブ書式版 1 は据え置き。** 封蝋はエンベロープ層で、中身にもワイヤ語彙にも触れない。
- **配布版（TUI）据え置き。** Seal-3 はサーバ完結で、配布物に何も入らない。TUI リリースのタグは打たない。
- **web `?v=` 据え置き。** `web/` を触らないので更新不要（前進するのは Seal-4）。
- **本番デプロイあり**（§7）。ここまでで「押された封蝋を第三者が検証できる」状態になる。

---

## 9. 次段への申し送り（Seal-4）

- `sealVerdict(envelope, keys, {sha256, verify})` → `'unsealed' | 'verified' | 'tampered' | 'unsupported' | 'unknown_key'`。判定順は概観 §4 の Seal-4 節に確定済み。
- **`alg !== "Ed25519"` は `unsupported`**。`verifySeal` は `alg` を見ないので、上位で弾く。
- **disputed は `text_a` を `sha256(texts[0])` と、`text_b` を `sha256(texts[1])` と突き合わせる。** Seal-2 の呼び出し側（`_handleTestimony` が `verdict.idA`/`idB` を渡す 1 行）はテストで守られていないので、その順序違いをここで構造的に捕まえる（`fd7e547` のレビューで判明）。
- **fetch は 2 本**（`/archive/:id` と `/recorder/keys`）。**独自ヘッダを付けないこと**——付けると preflight が必要になり、本段の「`OPTIONS` を作らない」判断が崩れる（§2.2）。
- 失敗時フォールバック（ネットワークエラー・404・JSON パース失敗）の挙動を指示書側で確定させること。web 初の `fetch` で踏襲先が無い。
- **既存アーカイブは `unsealed`**（封蝋導入前に綴じた記録）。失敗ではない。§7-5 より前の記録がすべてこれに当たる。

---

## 10. 不変の原則（本段が守るもの）

1. **検証できない署名に価値はない**（概観 不変の原則 7）。本段までが一区切りで、ここで初めて封蝋が意味を持つ。
2. **鍵は生成の形まで含めて検証する**（§1）。node と workerd で JWK が食い違う——「動くはず」で本番へ入れない。
3. **公開情報は対等**（版図）。書庫も鍵も `*` で開く。綴じ込みだけが特権。
4. **鍵が無いのは正常、壊れているのは異常**。前者も後者も応答は `{ keys: [] }` で揃えるが、後者はログに残す。Seal-2 の degradation と系として整合させる。
5. **継ぎ目だけ空け、作り込まない**（プリンシパル設計 §5）。鍵は配列で返すが、ローテーションも複数記録係も今は書かない。
6. **過ぎたるは及ばざる**。preflight が不要なら `OPTIONS` は作らない。
