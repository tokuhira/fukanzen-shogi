import { describe, it, expect } from "vitest";
import { SELF, env, runInDurableObject } from "cloudflare:test";

// ── 記録係三段目 Seal-3（鍵の公開口と CORS） ────────────────────────────────
// Worker のルーティングを SELF.fetch で叩く統合テスト。DO を経由する
// /room/:key も含めて Worker 全体を検証する（実装指示書 §6.1）。

// ⚠️ TEST 専用の鍵（Seal-1/Seal-2 と同一）。本番の RECORDER_SIGNING_KEY には
// 絶対に使わない。
const TEST_JWK_OBJ = {
  kty: "OKP", key_ops: ["sign"], alg: "EdDSA", ext: true, crv: "Ed25519",
  x: "s2qbZpbIC74LjfMin-RcE6Fqe99evbEkvbGil7LBSBE",
  d: "yBaW4fmzx56XvAxo17HpRyrXlLt8RwEk7pfSZO0CGCg",
};
const TEST_JWK = JSON.stringify(TEST_JWK_OBJ);
const TEST_KID = "1797cb4996f48595";

// env は共有オブジェクト。各テストで明示的に設定/削除する（room-seal.test.ts と同じ作法）。
function setSecret(v: string | null): void {
  if (v === null) delete (env as any).RECORDER_SIGNING_KEY;
  else (env as any).RECORDER_SIGNING_KEY = v;
}

describe("GET /recorder/keys", () => {
  it("a. 鍵ありで 1 件返り、kid・alg・public_key が揃う", async () => {
    setSecret(TEST_JWK);
    const res = await SELF.fetch("https://example.com/recorder/keys");
    expect(res.status).toBe(200);
    const body: any = await res.json();
    expect(body.keys.length).toBe(1);
    expect(body.keys[0].kid).toBe(TEST_KID);
    expect(body.keys[0].alg).toBe("Ed25519");
    expect(body.keys[0].public_key).toBe(TEST_JWK_OBJ.x);
  });

  it("b. 鍵なしで空配列（200・404 ではない）", async () => {
    setSecret(null);
    const res = await SELF.fetch("https://example.com/recorder/keys");
    expect(res.status).toBe(200);
    const body: any = await res.json();
    expect(body.keys).toEqual([]);
  });

  it("c. 壊れた鍵でも 200 + 空配列（500 にならない）", async () => {
    setSecret("not json{");
    const res = await SELF.fetch("https://example.com/recorder/keys");
    expect(res.status).toBe(200);
    const body: any = await res.json();
    expect(body.keys).toEqual([]);
  });

  it("d. CORS ヘッダが付く", async () => {
    setSecret(TEST_JWK);
    const res = await SELF.fetch("https://example.com/recorder/keys");
    expect(res.headers.get("Access-Control-Allow-Origin")).toBe("*");
  });

  it("g. Cache-Control が付く", async () => {
    setSecret(TEST_JWK);
    const res = await SELF.fetch("https://example.com/recorder/keys");
    expect(res.headers.get("Cache-Control")).toBe("public, max-age=300");
  });
});

describe("GET /archive/:id（既存エンドポイントの回帰）", () => {
  it("e. ⚠️ 200 応答に CORS ヘッダが付き、本体は壊れない", async () => {
    setSecret(null);
    const envelope = { finalized: true, text: "本文", archived_at: "2026-08-22T00:00:00.000Z", witnesses: 2 };
    await (env as any).ARCHIVES.put("test-archive-id", JSON.stringify(envelope));
    const res = await SELF.fetch("https://example.com/archive/test-archive-id");
    expect(res.status).toBe(200);
    expect(res.headers.get("Access-Control-Allow-Origin")).toBe("*");
    expect(res.headers.get("Content-Type")).toBe("application/json");
    const body: any = await res.json();
    expect(body).toEqual(envelope);
  });

  it("f. ⚠️ 404 応答にも CORS ヘッダが付く", async () => {
    const res = await SELF.fetch("https://example.com/archive/does-not-exist");
    expect(res.status).toBe(404);
    expect(res.headers.get("Access-Control-Allow-Origin")).toBe("*");
  });
});

describe("既存ルーティングの非回帰", () => {
  it("h. 未知トークンの /watch/:token は 404、未知パスも 404", async () => {
    const watchRes = await SELF.fetch("https://example.com/watch/no-such-token");
    expect(watchRes.status).toBe(404);
    const nopeRes = await SELF.fetch("https://example.com/nope");
    expect(nopeRes.status).toBe(404);
  });
});

describe("Seal-2 との突き合わせ（統合）", () => {
  it("i. 封蝋を押した鍵の kid と /recorder/keys の kid が一致する", async () => {
    setSecret(TEST_JWK);
    const room = (env as any).ROOM.get((env as any).ROOM.idFromName("seal3-integration"));
    const id = await runInDurableObject(room, async (i: any) => await i._archiveFinalized("本文", 2));
    const archived: any = await (env as any).ARCHIVES.get(id, "json");
    expect(archived.seal.kid).toBe(TEST_KID);

    const res = await SELF.fetch("https://example.com/recorder/keys");
    const body: any = await res.json();
    expect(body.keys[0].kid).toBe(archived.seal.kid);
  });
});
