import { describe, it, expect } from "vitest";
import { env, runInDurableObject } from "cloudflare:test";
import { verifySeal, fromBase64Url } from "../src/logic";
import type { SpectateRecord } from "../src/logic";

// ── 封蝋の配線（記録係三段目 Seal-2・room.ts） ──────────────────────────────
// これはこのプロジェクト初の DO テスト。runInDurableObject で GameRoom の
// 内部メソッドを直接呼ぶ（WebSocket を張って対局を進める必要はない）。

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

const DISPUTED_TEXT_A_HASH = "559aead08264d5795d3909718cdd05abd49572e84fe55590eef31a88a08fdffd";
const DISPUTED_TEXT_B_HASH = "df7e70e5021544f4834bbee64a9e3789febc4be81470df629cad6ddb03320a5c";

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

describe("_archiveFinalized（封蝋の配線）", () => {
  it("a. 鍵ありで finalized に封蝋が乗り、検証が通る", async () => {
    setSecret(TEST_JWK);
    const id = await runInDurableObject(room("seal-finalized-ok"), async (i: any) =>
      await i._archiveFinalized(SAMPLE_TEXT, 2));
    const stored = await readArchive(id);
    expect(stored.seal.alg).toBe("Ed25519");
    expect(stored.seal.kid).toBe(TEST_KID);
    expect(await verifySeal(RAW_PUB, stored.seal)).toBe(true);
  });

  it("b. 鍵なしで従来どおりの envelope（degradation・seal なし）", async () => {
    setSecret(null);
    const id = await runInDurableObject(room("seal-finalized-nokey"), async (i: any) =>
      await i._archiveFinalized(SAMPLE_TEXT, 2));
    const stored = await readArchive(id);
    expect(stored.seal).toBeUndefined();
    expect(stored.finalized).toBe(true);
    expect(stored.text).toBe(SAMPLE_TEXT);
    expect(typeof stored.archived_at).toBe("string");
    expect(stored.witnesses).toBe(2);
  });

  it("c. ⚠️ 壊れた Secret でも綴じは成立する（本段の本命・§1 の回帰固定）", async () => {
    setSecret("not json{");
    const id = await runInDurableObject(room("seal-broken"), async (i: any) =>
      await i._archiveFinalized("本文", 2));
    const stored = await readArchive(id);
    expect(stored).not.toBeNull();          // ← 綴じられている
    expect(stored.text).toBe("本文");
    expect(stored.witnesses).toBe(2);
    expect(stored.seal).toBeUndefined();    // ← 封蝋だけが無い
  });

  it("f. witnesses:1 経路にも封蝋が乗る", async () => {
    setSecret(TEST_JWK);
    const id = await runInDurableObject(room("seal-solo-witness"), async (i: any) =>
      await i._archiveFinalized(SAMPLE_TEXT, 1));
    const stored = await readArchive(id);
    expect(stored.seal.statement.split("\n")).toContain("witnesses 1");
  });

  it("g. statement の archived_at が envelope の archived_at と一致する（DO 経由の確認）", async () => {
    setSecret(TEST_JWK);
    const id = await runInDurableObject(room("seal-archived-at-match"), async (i: any) =>
      await i._archiveFinalized(SAMPLE_TEXT, 2));
    const stored = await readArchive(id);
    const archivedAtLine = stored.seal.statement
      .split("\n")
      .find((line: string) => line.startsWith("archived_at "));
    expect(archivedAtLine).toBe(`archived_at ${stored.archived_at}`);
  });

  it("h. 平文 witnesses を書き換えても statement は witnesses 2 のまま（食い違いが残る）", async () => {
    setSecret(TEST_JWK);
    const id = await runInDurableObject(room("seal-tamper"), async (i: any) =>
      await i._archiveFinalized(SAMPLE_TEXT, 2));
    const stored = await readArchive(id);
    stored.witnesses = 1;                                     // 改竄を模す
    expect(await verifySeal(RAW_PUB, stored.seal)).toBe(true); // 署名自体は有効
    expect(stored.seal.statement.split("\n")).toContain("witnesses 2");
    expect(stored.witnesses).toBe(1);                          // ← 食い違いが残る
  });
});

describe("_archiveDisputed（封蝋の配線）", () => {
  it("d. disputed にも封蝋が乗り、両ハッシュがその順で並ぶ", async () => {
    setSecret(TEST_JWK);
    const id = await runInDurableObject(room("seal-disputed-ok"), async (i: any) =>
      await i._archiveDisputed(["A", "B"], { a: DISPUTED_TEXT_A_HASH, b: DISPUTED_TEXT_B_HASH }));
    const stored = await readArchive(id);
    const lines: string[] = stored.seal.statement.split("\n");
    expect(lines).toContain(`text_a ${DISPUTED_TEXT_A_HASH}`);
    expect(lines).toContain(`text_b ${DISPUTED_TEXT_B_HASH}`);
    expect(lines.indexOf(`text_a ${DISPUTED_TEXT_A_HASH}`))
      .toBeLessThan(lines.indexOf(`text_b ${DISPUTED_TEXT_B_HASH}`));
    expect(await verifySeal(RAW_PUB, stored.seal)).toBe(true);
  });

  it("e. disputed も壊れた Secret で綴じは成立する", async () => {
    setSecret("not json{");
    const id = await runInDurableObject(room("seal-disputed-broken"), async (i: any) =>
      await i._archiveDisputed(["A", "B"], { a: DISPUTED_TEXT_A_HASH, b: DISPUTED_TEXT_B_HASH }));
    const stored = await readArchive(id);
    expect(stored).not.toBeNull();
    expect(stored.texts).toEqual(["A", "B"]);
    expect(stored.seal).toBeUndefined();
  });
});

describe("_archiveFragment（対象外・概観 §7）", () => {
  it("i. fragment には封蝋を押さない", async () => {
    setSecret(TEST_JWK);
    const record: SpectateRecord = {
      version: { rule: "0.7", protocol: 5, app: "0.13.0" },
      initial_sfen: "startpos",
      turns: [{ s: "7g7f", g: "3c3d" }],
      result: null,
      archived: false,
      recording: true,
    };
    const id = await runInDurableObject(room("seal-fragment"), async (i: any) =>
      await i._archiveFragment(record));
    const stored = await readArchive(id);
    expect(stored.seal).toBeUndefined();
  });
});
