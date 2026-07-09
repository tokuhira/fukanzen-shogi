import { describe, it, expect, beforeAll } from "vitest";
import { movesFromSquare, dropsOfKind, buildTargetMap, resolveTarget } from "../move-input.js";
import { parseUsi } from "../usi.js";
import { loadEngine } from "./wasm-loader.js";

// 8八角が3三へ入れる局面（8h3c と 8h3c+ が両立）。実地で確認済み。
const SFEN = "lnsgkgsnl/1r5b1/pppppp1pp/6p2/9/2P6/PP1PPPPPP/1B5R1/LNSGKGSNL b - 5";
let senteMoves;
beforeAll(async () => {
  const engine = await loadEngine();
  senteMoves = JSON.parse(engine.legal_actions(SFEN, "sente")).map(parseUsi);
});

describe("move-input（着手組み立ての純粋計算）", () => {
  it("movesFromSquare は盤上 from の手だけに絞る", () => {
    const ms = movesFromSquare(senteMoves, 8, 8); // 8八角
    expect(ms.length).toBeGreaterThan(0);
    expect(ms.every(m => !m.isDrop && m.from[0] === 8 && m.from[1] === 8)).toBe(true);
  });

  it("dropsOfKind は打ちが無ければ空（この局面は持ち駒なし）", () => {
    expect(dropsOfKind(senteMoves, "P")).toEqual([]);
  });

  it("buildTargetMap は到達点ごとに options を畳む", () => {
    const tm = buildTargetMap(movesFromSquare(senteMoves, 8, 8));
    const entry = tm.get("3,3"); // 3三へ
    expect(entry).toBeTruthy();
    expect(entry.options.some(o => o.promote)).toBe(true);
    expect(entry.options.some(o => !o.promote)).toBe(true);
  });

  it("resolveTarget: 成不成が両立→promptPromotion", () => {
    const tm = buildTargetMap(movesFromSquare(senteMoves, 8, 8));
    const a = resolveTarget(tm, 3, 3);
    expect(a.kind).toBe("promptPromotion");
    expect(a.toSquare).toEqual([3, 3]);
    expect(a.options.length).toBe(2);
  });

  it("resolveTarget: 一方のみ→confirm（usi を返す）", () => {
    // 2七歩を2六へ（不成のみ）
    const tm = buildTargetMap(movesFromSquare(senteMoves, 2, 7));
    const a = resolveTarget(tm, 2, 6);
    expect(a.kind).toBe("confirm");
    expect(a.usi).toBe("2g2f");
  });

  it("resolveTarget: 到達点でない→deselect", () => {
    const tm = buildTargetMap(movesFromSquare(senteMoves, 2, 7));
    expect(resolveTarget(tm, 5, 5)).toEqual({ kind: "deselect" });
  });
});
