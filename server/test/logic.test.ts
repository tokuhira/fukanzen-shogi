import { describe, it, expect } from "vitest";
import {
  routeDecision,
  evaluateTestimonies,
  isValidTestimonyText,
  sha256Hex,
  buildFinalizedEnvelope,
  buildDisputedEnvelope,
  buildFragmentEnvelope,
  shouldArchiveFragment,
  canAppendTurn,
  MAX_TURNS,
  MAX_ARCHIVE_TEXT_BYTES,
  toBase64Url,
  fromBase64Url,
  kidFromRawPublicKey,
  buildSealStatement,
  loadRecorderKey,
  sealFinalized,
  sealDisputed,
  verifySeal,
  type SpectateRecord,
  type RecorderKey,
} from "../src/logic";

// ── 秘匿境界（淀川第三歩 §1-B・記録係二段目 §10） ────────────────────────────
// 観戦者の入力は型を問わず無条件破棄。プレイヤーの対局チャネル（commit/reveal/
// ack/hello/reconnect）は観戦者へ絶対に fan-out されない。v0.10.1 の退行
// （観戦者の request_reset が対局を壊した）を固定する回帰テストでもある。
describe("routeDecision（秘匿境界・routing）", () => {
  it("観戦者からの入力はどんな型でも discard（v0.10.1 の退行防止を固定）", () => {
    for (const type of ["commit", "reveal", "ack", "hello", "request_reset", "record_testimony", "spectate_turn", ""]) {
      expect(routeDecision(true, type)).toBe("discard");
    }
  });

  it("spectate_meta/turn/result は spectate_fanout", () => {
    expect(routeDecision(false, "spectate_meta")).toBe("spectate_fanout");
    expect(routeDecision(false, "spectate_turn")).toBe("spectate_fanout");
    expect(routeDecision(false, "spectate_result")).toBe("spectate_fanout");
  });

  it("記録係の招待・二証人・request_reset は server_handled（観戦者へ中継しない）", () => {
    for (const type of ["request_reset", "record_invite", "record_accept", "record_decline", "record_testimony"]) {
      expect(routeDecision(false, type)).toBe("server_handled");
    }
  });

  it("対局チャネル（commit/reveal/ack/hello/reconnect）は other_player_only", () => {
    for (const type of ["commit", "reveal", "ack", "hello", "reconnect", "reconnect_ack", "abort"]) {
      expect(routeDecision(false, type)).toBe("other_player_only");
    }
  });
});

// ── 二証人の交差確認（記録係二段目 §3） ─────────────────────────────────────
describe("evaluateTestimonies（二証人の評決）", () => {
  it("同一本文 → witnesses:2 で一致（同じ commit-reveal を独立に再生した二者はバイト一致する）", async () => {
    const text = "同じ棋譜のはず";
    const verdict = await evaluateTestimonies(text, text);
    expect(verdict.kind).toBe("matched");
    if (verdict.kind === "matched") {
      expect(verdict.witnesses).toBe(2);
      expect(verdict.id).toBe(await sha256Hex(text));
    }
  });

  it("食い違う本文 → disputed（裁定しない。各自のハッシュを返すのみ）", async () => {
    const textA = "先手の言い分";
    const textB = "後手の言い分";
    const verdict = await evaluateTestimonies(textA, textB);
    expect(verdict.kind).toBe("disputed");
    if (verdict.kind === "disputed") {
      expect(verdict.idA).toBe(await sha256Hex(textA));
      expect(verdict.idB).toBe(await sha256Hex(textB));
      expect(verdict.idA).not.toBe(verdict.idB);
    }
  });

  it("空文字を含む証言は rejected（綴じない。後で断片綴じにフォールバックしうる）", async () => {
    expect((await evaluateTestimonies("", "本文")).kind).toBe("rejected");
    expect((await evaluateTestimonies("本文", "")).kind).toBe("rejected");
  });

  it("上限（MAX_ARCHIVE_TEXT_BYTES）を超える証言は rejected", async () => {
    const oversized = "x".repeat(MAX_ARCHIVE_TEXT_BYTES + 1);
    const normal = "normal";
    expect((await evaluateTestimonies(oversized, normal)).kind).toBe("rejected");
    expect((await evaluateTestimonies(normal, oversized)).kind).toBe("rejected");
  });

  it("ちょうど上限のサイズは許容される（境界値）", async () => {
    const atLimit = "x".repeat(MAX_ARCHIVE_TEXT_BYTES);
    expect(isValidTestimonyText(atLimit)).toBe(true);
    expect(isValidTestimonyText(atLimit + "x")).toBe(false);
  });
});

// ── content-address（記録係一段目 §2・§7） ──────────────────────────────────
describe("sha256Hex（content-address）", () => {
  it("既知のテストベクタと一致する（空文字列の SHA-256）", async () => {
    // SHA-256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
    expect(await sha256Hex("")).toBe(
      "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    );
  });

  it("同じ本文は同じハッシュ、異なる本文は異なるハッシュを生む", async () => {
    const h1 = await sha256Hex("同じ棋譜");
    const h2 = await sha256Hex("同じ棋譜");
    const h3 = await sha256Hex("違う棋譜");
    expect(h1).toBe(h2);
    expect(h1).not.toBe(h3);
    expect(h1).toMatch(/^[0-9a-f]{64}$/);
  });
});

// ── envelope 構築（記録係一段目 §4-1・二段目 §3） ───────────────────────────
describe("envelope 構築", () => {
  it("確定局 envelope は finalized:true・witnesses を持つ", () => {
    const env = buildFinalizedEnvelope("本文", 2);
    expect(env.finalized).toBe(true);
    expect(env.text).toBe("本文");
    expect(env.witnesses).toBe(2);
    expect(typeof env.archived_at).toBe("string");
  });

  it("disputed envelope は両証言を texts に保持し、裁定を含まない", () => {
    const env = buildDisputedEnvelope(["Aの言い分", "Bの言い分"]);
    expect(env.finalized).toBe(false);
    expect(env.disputed).toBe(true);
    expect(env.texts).toEqual(["Aの言い分", "Bの言い分"]);
  });

  it("放棄断片 envelope は record の中身のみを保持する（version/turns/result）", () => {
    const record: SpectateRecord = {
      version: { rule: "0.6", protocol: 4, app: "0.11.1" },
      initial_sfen: "startpos",
      turns: [{ s: "7g7f", g: "3c3d" }],
      result: null,
      archived: false,
      recording: true,
    };
    const env = buildFragmentEnvelope(record);
    expect(env.finalized).toBe(false);
    expect(env.record.turns).toEqual(record.turns);
    expect(env.record.initial_sfen).toBe("startpos");
    // archived/recording はレコード管理用の内部フラグであり、envelope には含めない。
    expect(env.record).not.toHaveProperty("archived");
    expect(env.record).not.toHaveProperty("recording");
  });
});

// ── 「綴じてから拭く」ゲート（記録係一段目 §4・二段目 §4） ─────────────────
describe("shouldArchiveFragment（綴じてから拭くゲート）", () => {
  it("招かれ（recording）・未綴じ・着手ありのときのみ true", () => {
    expect(shouldArchiveFragment(true, false, 1)).toBe(true);
  });

  it("未招待なら終局しても揮発してよい（false）", () => {
    expect(shouldArchiveFragment(false, false, 5)).toBe(false);
  });

  it("既に綴じ済みなら二重に綴じない（false）", () => {
    expect(shouldArchiveFragment(true, true, 5)).toBe(false);
  });

  it("着手が一つもなければ綴じる意味がない（false）", () => {
    expect(shouldArchiveFragment(true, false, 0)).toBe(false);
  });
});

describe("canAppendTurn（spectate_turn の上限ゲート）", () => {
  it("MAX_TURNS 未満なら追記できる", () => {
    expect(canAppendTurn(MAX_TURNS - 1)).toBe(true);
  });

  it("MAX_TURNS に達したら追記できない（境界値）", () => {
    expect(canAppendTurn(MAX_TURNS)).toBe(false);
    expect(canAppendTurn(MAX_TURNS + 1)).toBe(false);
  });
});

// ── 封蝋（記録係三段目 Seal-1・概観 §2.1） ───────────────────────────────────
// ⚠️ TEST 専用の鍵。本番の RECORDER_SIGNING_KEY には絶対に使わない
// （実装指示書 §8.1・workerd 上で実測検証済みの golden 値）。
const TEST_JWK = {
  kty: "OKP", key_ops: ["sign"], alg: "EdDSA", ext: true, crv: "Ed25519",
  x: "s2qbZpbIC74LjfMin-RcE6Fqe99evbEkvbGil7LBSBE",
  d: "yBaW4fmzx56XvAxo17HpRyrXlLt8RwEk7pfSZO0CGCg",
};
const TEST_KID = "1797cb4996f48595";

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
const SAMPLE_ID = "e730294a6fcd51c0f5f9e7bdfb4de3cb91bee56ae4c10c74ab1630d79b0902da";
const SAMPLE_ARCHIVED_AT = "2026-08-22T12:34:56.789Z";

const GOLDEN_STATEMENT_FINALIZED = [
  "fukanzen-shogi-seal 1",
  "kind finalized",
  `id ${SAMPLE_ID}`,
  "witnesses 2",
  `archived_at ${SAMPLE_ARCHIVED_AT}`,
  `kid ${TEST_KID}`,
].join("\n");
const GOLDEN_SIG_FINALIZED =
  "AnyomxmOegNdrBmOxL5MNxM_eHWP37lPXvaKRCJzMoo9BWk2H6lNH00AZUna3JOEY3MphGusMYIJLLXU5wXyCg";

const DISPUTED_ID = "018f3e7a-0000-7000-8000-000000000000";
const DISPUTED_TEXT_A_HASH = "559aead08264d5795d3909718cdd05abd49572e84fe55590eef31a88a08fdffd";
const DISPUTED_TEXT_B_HASH = "df7e70e5021544f4834bbee64a9e3789febc4be81470df629cad6ddb03320a5c";
const GOLDEN_STATEMENT_DISPUTED = [
  "fukanzen-shogi-seal 1",
  "kind disputed",
  `id ${DISPUTED_ID}`,
  `text_a ${DISPUTED_TEXT_A_HASH}`,
  `text_b ${DISPUTED_TEXT_B_HASH}`,
  `archived_at ${SAMPLE_ARCHIVED_AT}`,
  `kid ${TEST_KID}`,
].join("\n");
const GOLDEN_SIG_DISPUTED =
  "dXfCD-uNKVGJ7gLOYzinOsIopC44g-YZJietqt0Nknoj3SOBXdj1D8f5sEriWlk384NgsxdO8W5RxuFi_lD2Cg";

describe("toBase64Url / fromBase64Url（base64url）", () => {
  it("往復する（fromBase64Url(toBase64Url(x)) === x）", () => {
    const bytes = new Uint8Array([0, 1, 2, 253, 254, 255, 128, 64, 10]);
    expect(fromBase64Url(toBase64Url(bytes))).toEqual(bytes);
  });

  it("パディング長 0・1・2 の各ケースで往復する（3n/3n+1/3n+2 バイト）", () => {
    const cases = [
      new Uint8Array([0xfb, 0xff, 0xbf]), // 3 バイト → 標準base64のパディング0
      new Uint8Array([0xfb]),             // 1 バイト → パディング2
      new Uint8Array([0xfb, 0xff]),       // 2 バイト → パディング1
    ];
    for (const bytes of cases) {
      const url = toBase64Url(bytes);
      expect(url).not.toMatch(/=/);
      expect(fromBase64Url(url)).toEqual(bytes);
    }
  });

  it("+ と / を含むバイト列は - と _ になる（標準base64だと \"+/+/\"）", () => {
    const bytes = new Uint8Array([0xfb, 0xff, 0xbf]);
    expect(toBase64Url(bytes)).toBe("-_-_");
  });
});

describe("kidFromRawPublicKey（kid）", () => {
  it("TEST_JWK.x から TEST_KID が決定的に出る", async () => {
    const raw = fromBase64Url(TEST_JWK.x);
    expect(await kidFromRawPublicKey(raw)).toBe(TEST_KID);
    expect(await kidFromRawPublicKey(raw)).toBe(TEST_KID);
  });
});

describe("buildSealStatement（golden・バイト完全一致）", () => {
  it("finalized の statement が golden 文字列と完全一致する", () => {
    const statement = buildSealStatement(
      { kind: "finalized", id: SAMPLE_ID, witnesses: 2 },
      SAMPLE_ARCHIVED_AT,
      TEST_KID,
    );
    expect(statement).toBe(GOLDEN_STATEMENT_FINALIZED);
  });

  it("disputed の statement が golden 文字列と完全一致する", () => {
    const statement = buildSealStatement(
      { kind: "disputed", id: DISPUTED_ID, text_a: DISPUTED_TEXT_A_HASH, text_b: DISPUTED_TEXT_B_HASH },
      SAMPLE_ARCHIVED_AT,
      TEST_KID,
    );
    expect(statement).toBe(GOLDEN_STATEMENT_DISPUTED);
  });
});

describe("loadRecorderKey（鍵の読み込み・モジュール memo）", () => {
  it("Secret 未設定なら null（封蝋を省く degradation）", async () => {
    expect(await loadRecorderKey({})).toBeNull();
  });

  it("TEST_JWK の JSON を渡すと kid が TEST_KID になる", async () => {
    const key = await loadRecorderKey({ RECORDER_SIGNING_KEY: JSON.stringify(TEST_JWK) });
    expect(key).not.toBeNull();
    expect(key?.kid).toBe(TEST_KID);
  });

  it("壊れた JSON は throw する（設定ミスを黙って握り潰さない）", async () => {
    await expect(loadRecorderKey({ RECORDER_SIGNING_KEY: "not json{" })).rejects.toThrow();
  });

  it("同じ Secret で二度呼ぶと同じオブジェクトが返る（memo が効いている）", async () => {
    const secret = JSON.stringify(TEST_JWK);
    const [key1, key2] = await Promise.all([
      loadRecorderKey({ RECORDER_SIGNING_KEY: secret }),
      loadRecorderKey({ RECORDER_SIGNING_KEY: secret }),
    ]);
    expect(key1).toBe(key2);
  });
});

describe("sealFinalized / sealDisputed（署名 golden・一方向合成）", () => {
  it("sealFinalized が golden の sig と一致する", async () => {
    const key = await loadRecorderKey({ RECORDER_SIGNING_KEY: JSON.stringify(TEST_JWK) });
    const envelope = { finalized: true as const, text: SAMPLE_TEXT, archived_at: SAMPLE_ARCHIVED_AT, witnesses: 2 };
    const sealed = await sealFinalized(envelope, SAMPLE_ID, key);
    expect(sealed.seal?.statement).toBe(GOLDEN_STATEMENT_FINALIZED);
    expect(sealed.seal?.sig).toBe(GOLDEN_SIG_FINALIZED);
    expect(sealed.seal?.alg).toBe("Ed25519");
    expect(sealed.seal?.kid).toBe(TEST_KID);
  });

  it("sealDisputed が golden の sig と一致する", async () => {
    const key = await loadRecorderKey({ RECORDER_SIGNING_KEY: JSON.stringify(TEST_JWK) });
    const envelope = {
      finalized: false as const, disputed: true as const,
      texts: ["A", "B"] as [string, string], archived_at: SAMPLE_ARCHIVED_AT,
    };
    const sealed = await sealDisputed(
      envelope, DISPUTED_ID, { a: DISPUTED_TEXT_A_HASH, b: DISPUTED_TEXT_B_HASH }, key,
    );
    expect(sealed.seal?.statement).toBe(GOLDEN_STATEMENT_DISPUTED);
    expect(sealed.seal?.sig).toBe(GOLDEN_SIG_DISPUTED);
  });

  // 「今」ではありえない sentinel を置くのが要。buildFinalizedEnvelope が生成した
  // ままの archived_at で比べると、nowIso() を呼び直すバグ実装でもテストが通って
  // しまう——toISOString() はミリ秒解像度で、間に I/O が無い二度の生成は必ず同じ
  // ミリ秒に落ちるから（実測 200/200 ですり抜けた）。sentinel なら 200/200 捕まえる。
  const NOT_NOW = "1999-01-01T00:00:00.000Z";

  it("sealFinalized は envelope の archived_at をそのまま使う（概観 §2.1 の回帰固定）", async () => {
    const key = await loadRecorderKey({ RECORDER_SIGNING_KEY: JSON.stringify(TEST_JWK) });
    const envelope = { ...buildFinalizedEnvelope("本文", 2), archived_at: NOT_NOW };
    const sealed = await sealFinalized(envelope, "dummy-id", key);
    expect(sealed.seal?.statement.split("\n")).toContain(`archived_at ${NOT_NOW}`);
  });

  it("sealDisputed も envelope の archived_at をそのまま使う", async () => {
    const key = await loadRecorderKey({ RECORDER_SIGNING_KEY: JSON.stringify(TEST_JWK) });
    const envelope = { ...buildDisputedEnvelope(["A", "B"]), archived_at: NOT_NOW };
    const sealed = await sealDisputed(envelope, "dummy-id", { a: "hashA", b: "hashB" }, key);
    expect(sealed.seal?.statement.split("\n")).toContain(`archived_at ${NOT_NOW}`);
  });

  it("witnesses も envelope から読む（witnesses:1 の envelope は statement も witnesses 1）", async () => {
    const key = await loadRecorderKey({ RECORDER_SIGNING_KEY: JSON.stringify(TEST_JWK) });
    const envelope = buildFinalizedEnvelope("本文", 1);
    const sealed = await sealFinalized(envelope, "dummy-id", key);
    expect(sealed.seal?.statement.split("\n")).toContain("witnesses 1");
  });

  it("鍵が null なら封蝋なしで素通しする", async () => {
    const envelope = buildFinalizedEnvelope("本文", 2);
    const sealed = await sealFinalized(envelope, "dummy-id", null);
    expect(sealed.seal).toBeUndefined();
    expect(sealed).toEqual(envelope);

    const disputedEnvelope = buildDisputedEnvelope(["A", "B"]);
    const sealedDisputed = await sealDisputed(
      disputedEnvelope, "dummy-id", { a: "hashA", b: "hashB" }, null,
    );
    expect(sealedDisputed.seal).toBeUndefined();
    expect(sealedDisputed).toEqual(disputedEnvelope);
  });
});

describe("verifySeal（検証）", () => {
  async function sealedFinalizedFixture(key: RecorderKey) {
    const envelope = { finalized: true as const, text: SAMPLE_TEXT, archived_at: SAMPLE_ARCHIVED_AT, witnesses: 2 };
    return sealFinalized(envelope, SAMPLE_ID, key);
  }

  it("正しい封蝋は true", async () => {
    const key = await loadRecorderKey({ RECORDER_SIGNING_KEY: JSON.stringify(TEST_JWK) });
    const sealed = await sealedFinalizedFixture(key!);
    expect(await verifySeal(key!.rawPublicKey, sealed.seal!)).toBe(true);
  });

  it("statement を 1 文字変えると false", async () => {
    const key = await loadRecorderKey({ RECORDER_SIGNING_KEY: JSON.stringify(TEST_JWK) });
    const sealed = await sealedFinalizedFixture(key!);
    const tampered = { ...sealed.seal!, statement: sealed.seal!.statement.replace("witnesses 2", "witnesses 3") };
    expect(await verifySeal(key!.rawPublicKey, tampered)).toBe(false);
  });

  it("sig を壊すと false", async () => {
    const key = await loadRecorderKey({ RECORDER_SIGNING_KEY: JSON.stringify(TEST_JWK) });
    const sealed = await sealedFinalizedFixture(key!);
    const brokenSig = sealed.seal!.sig.slice(0, -4) + (sealed.seal!.sig.slice(-4) === "AAAA" ? "BBBB" : "AAAA");
    const tampered = { ...sealed.seal!, sig: brokenSig };
    expect(await verifySeal(key!.rawPublicKey, tampered)).toBe(false);
  });

  it("別の鍵の raw 公開鍵では false", async () => {
    const key = await loadRecorderKey({ RECORDER_SIGNING_KEY: JSON.stringify(TEST_JWK) });
    const sealed = await sealedFinalizedFixture(key!);
    const otherKeyPair = (await crypto.subtle.generateKey({ name: "Ed25519" }, true, ["sign", "verify"])) as CryptoKeyPair;
    const otherRaw = new Uint8Array(await crypto.subtle.exportKey("raw", otherKeyPair.publicKey));
    expect(await verifySeal(otherRaw, sealed.seal!)).toBe(false);
  });
});

describe("既存 envelope の非回帰（封じるまでは seal が現れない）", () => {
  it("buildFinalizedEnvelope の出力に seal が現れない", () => {
    const env = buildFinalizedEnvelope("本文", 2);
    expect(env).not.toHaveProperty("seal");
  });

  it("buildDisputedEnvelope の出力に seal が現れない", () => {
    const env = buildDisputedEnvelope(["A", "B"]);
    expect(env).not.toHaveProperty("seal");
  });

  it("buildFragmentEnvelope の出力に seal が現れない", () => {
    const record: SpectateRecord = {
      version: { rule: "0.7", protocol: 5, app: "0.13.0" },
      initial_sfen: "startpos",
      turns: [],
      result: null,
      archived: false,
      recording: false,
    };
    const env = buildFragmentEnvelope(record);
    expect(env).not.toHaveProperty("seal");
  });
});
