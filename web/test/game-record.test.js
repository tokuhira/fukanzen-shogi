import { describe, it, expect, beforeAll } from "vitest";
import { emptyRecord, appendTurn, truncateTo, buildFromPlies } from "../game-record.js";
import { loadEngine, loadNotation } from "./wasm-loader.js";

const INITIAL = "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1";
let resolvePly, usiToText;
beforeAll(async () => {
  const engine = await loadEngine();
  const notation = await loadNotation();
  resolvePly = (sfen, s, g) => JSON.parse(engine.resolve_ply(sfen, s, g));
  usiToText  = (usi, sfen, side) =>
    (side === "sente" ? "☗" : "☖") +
    notation.ja_notation(usi, side, engine.legal_actions(sfen, side), sfen);
});

describe("game-record（純粋遷移）", () => {
  it("emptyRecord は初期局面 1 本・空の events/plies", () => {
    const r = emptyRecord(INITIAL);
    expect(r.sfens).toEqual([INITIAL]);
    expect(r.events).toEqual([]);
    expect(r.plies).toEqual([]);
  });

  it("appendTurn で sfens/events/plies が 1 組手ぶん育ち、棋譜が導出される", () => {
    const r = appendTurn(emptyRecord(INITIAL), "7g7f", "3c3d", resolvePly, usiToText);
    expect(r.sfens.length).toBe(2);
    expect(r.events.length).toBe(1);
    expect(r.plies.length).toBe(1);
    expect(r.plies[0].sText).toBe("☗７六歩");
    expect(r.plies[0].gText).toBe("☖３四歩");
  });

  it("appendTurn は引数の record を変更しない（不変）", () => {
    const base = emptyRecord(INITIAL);
    appendTurn(base, "7g7f", "3c3d", resolvePly, usiToText);
    expect(base.sfens.length).toBe(1);  // 元は不変
    expect(base.plies.length).toBe(0);
  });

  it("渡した sText/gText はそのまま使われる（再計算しない）", () => {
    const r = appendTurn(emptyRecord(INITIAL), "7g7f", "3c3d", resolvePly, usiToText, "S", "G");
    expect(r.plies[0].sText).toBe("S");
    expect(r.plies[0].gText).toBe("G");
  });

  it("truncateTo で n 組手＋局面 n+1 本に切り詰まる", () => {
    let r = buildFromPlies(INITIAL, [
      { sUsi: "7g7f", gUsi: "3c3d" },
      { sUsi: "2g2f", gUsi: "8c8d" },
    ], resolvePly, usiToText);
    expect(r.plies.length).toBe(2);
    const t = truncateTo(r, 1);
    expect(t.plies.length).toBe(1);
    expect(t.sfens.length).toBe(2);
    expect(t.events.length).toBe(1);
    expect(r.plies.length).toBe(2);  // 元は不変
  });

  it("buildFromPlies は plies 列から record を組み直す", () => {
    const r = buildFromPlies(INITIAL, [{ sUsi: "7g7f", gUsi: "3c3d" }], resolvePly, usiToText);
    expect(r.sfens.length).toBe(2);
    expect(r.plies[0].sText).toBe("☗７六歩");
  });

  it("不正な USI は resolvePly が ok:false を返し appendTurn が throw", () => {
    // resolve_ply は SFEN/USI の構文だけを検証し、着手の合法性は検証しない
    // （合法性は movegen の役割）。ここは構文的に不正な USI（file=0）で確実に
    // ok:false を踏む——実 Wasm で確認済み（"9i9a" 等の形だけ変な移動は
    // ok:true を返すため、それでは本テストの意図を満たさない）。
    expect(() => appendTurn(emptyRecord(INITIAL), "0000", "0000", resolvePly, usiToText)).toThrow();
  });
});
