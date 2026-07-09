import { describe, it, expect } from "vitest";
import { parseUsi, charToRank } from "../usi.js";

describe("charToRank", () => {
  it("'a' は 1、'i' は 9", () => {
    expect(charToRank("a")).toBe(1);
    expect(charToRank("i")).toBe(9);
  });
});

describe("parseUsi", () => {
  it("通常の移動（成りなし）", () => {
    const m = parseUsi("7g7f");
    expect(m.isDrop).toBe(false);
    expect(m.from).toEqual([7, charToRank("g")]);
    expect(m.to).toEqual([7, charToRank("f")]);
    expect(m.promote).toBe(false);
  });

  it("打ち", () => {
    const m = parseUsi("P*5e");
    expect(m.isDrop).toBe(true);
    expect(m.kind).toBe("P");
    expect(m.to).toEqual([5, charToRank("e")]);
  });

  it("成り（5文字）", () => {
    const m = parseUsi("2b2a+");
    expect(m.isDrop).toBe(false);
    expect(m.promote).toBe(true);
  });
});
