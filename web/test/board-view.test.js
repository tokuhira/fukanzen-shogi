import { describe, it, expect } from "vitest";
import { renderSvg, inputOverlay, revealOverlay } from "../board-view.js";

// board.js の消費者形と同じ {board:Map, handS, handG} を手書きで作る
// （position-view.test.js と同じ流儀）。Map は挿入順が描画順に効くので固定する。
function samplePos() {
  const board = new Map([
    ["7,7", { kind: "P", side: "s" }],
    ["8,8", { kind: "+B", side: "s" }],
    ["3,3", { kind: "P", side: "g" }],
  ]);
  return { board, handS: { P: 2, B: 1 }, handG: { P: 1 } };
}

function sampleOverlay() {
  return {
    board: [[3, 3]],
    selectedSquare: [7, 7],
    legalDots: new Set(["7,6"]),
    gHand: new Set(["P"]),
  };
}

describe("renderSvg", () => {
  it("後手駒は rotate(180,...) を含み、先手駒は含まない", () => {
    const svg = renderSvg(samplePos(), null);
    expect(svg).toContain('transform="rotate(180,');
    // 先手の歩（7,7）は非回転の <text> で描画される
    expect(svg).toMatch(/<text x="\d+" y="[\d.]+" text-anchor="middle" font-size="22" fill="#1a1a1a">歩<\/text>/);
  });

  it("成り駒は KANJI 経由で成り字形になる（+B → 馬）", () => {
    const svg = renderSvg(samplePos(), null);
    expect(svg).toContain(">馬<");
  });

  it("持ち駒は字形＋countStr、無ければ「なし」", () => {
    const svg = renderSvg(samplePos(), null);
    // handS: P:2 → 歩 + countStr(2)='二'、B:1 → 角（countStr(1)=''）
    expect(svg).toContain("歩二");
    expect(svg).toContain(">角<");
  });

  it("持ち駒が空なら「なし」", () => {
    const pos = samplePos();
    pos.handG = {};
    const svg = renderSvg(pos, null);
    expect(svg).toContain("なし");
  });

  it("overlay.legalDots は circle、overlay.selectedSquare は強調 rect", () => {
    const svg = renderSvg(samplePos(), sampleOverlay());
    expect(svg).toContain("<circle");
    expect(svg).toContain('fill-opacity="0.14"');
  });

  it("golden: 代表局面の SVG 出力を固定する（視覚回帰の錠）", () => {
    expect(renderSvg(samplePos(), sampleOverlay())).toMatchSnapshot();
  });
});

describe("inputOverlay（選択状態→overlay）", () => {
  it("盤上選択は selectedSquare を立て、legalTargets は墨点になる", () => {
    const lt = new Map([["7,6", { options: [] }], ["7,5", { options: [] }]]);
    const ov = inputOverlay({ selectedFrom: { board: [7, 7] }, inputStep: "sente", legalTargets: lt });
    expect(ov.selectedSquare).toEqual([7, 7]);
    expect(ov.legalDots).toEqual(new Set(["7,6", "7,5"]));
    expect(ov.sHand).toBeNull();
  });

  it("先手の持ち駒選択は sHand に入る", () => {
    const ov = inputOverlay({ selectedFrom: { hand: "P" }, inputStep: "sente", legalTargets: null });
    expect(ov.sHand).toEqual(new Set(["P"]));
    expect(ov.gHand).toBeNull();
    expect(ov.selectedSquare).toBeNull();
  });

  it("後手の持ち駒選択は gHand に入る", () => {
    const ov = inputOverlay({ selectedFrom: { hand: "P" }, inputStep: "gote", legalTargets: null });
    expect(ov.gHand).toEqual(new Set(["P"]));
    expect(ov.sHand).toBeNull();
  });

  it("選択なしなら空 overlay（board 空・全 null）", () => {
    const ov = inputOverlay({ selectedFrom: null, inputStep: null, legalTargets: null });
    expect(ov.selectedSquare).toBeNull();
    expect(ov.legalDots).toBeNull();
    expect(ov.board).toEqual([]);
  });
});

describe("revealOverlay（開示する組手→overlay）", () => {
  it("ply が無ければ null", () => {
    expect(revealOverlay(undefined)).toBeNull();
  });

  it("盤上の手は from/to、打ちは to のみ＋持ち駒 Set", () => {
    const ov = revealOverlay({ sUsi: "7g7f", gUsi: "P*5e" });
    // sente 7g7f: from[7,7] to[7,6] 両方 board に入る（drop でない）
    // gote  P*5e: to[5,5] のみ（from は null で除外）、gHand に P
    expect(ov.board).toContainEqual([7, 7]);
    expect(ov.board).toContainEqual([7, 6]);
    expect(ov.board).toContainEqual([5, 5]);
    expect(ov.gHand).toEqual(new Set(["P"]));
    expect(ov.sHand).toBeNull();
  });
});
