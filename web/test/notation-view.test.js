import { describe, it, expect, beforeAll } from "vitest";
import { usiToText } from "../notation-view.js";
import { loadEngine, loadNotation } from "./wasm-loader.js";

const INITIAL_SFEN =
  "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1";

let legalActions, jaNotation;
beforeAll(async () => {
  const engine   = await loadEngine();
  const notation = await loadNotation();
  legalActions = engine.legal_actions;
  jaNotation   = notation.ja_notation;
});

describe("usiToText（Wasm を跨ぐ糊）", () => {
  it("先手の 7g7f は ☗７六歩", () => {
    expect(usiToText("7g7f", INITIAL_SFEN, "sente", legalActions, jaNotation))
      .toBe("☗７六歩");
  });
  it("後手の接頭は ☖", () => {
    const t = usiToText("3c3d", INITIAL_SFEN, "gote", legalActions, jaNotation);
    expect(t.startsWith("☖")).toBe(true);
  });
});
