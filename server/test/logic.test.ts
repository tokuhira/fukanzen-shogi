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
  type SpectateRecord,
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
