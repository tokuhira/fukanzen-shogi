import { describe, it, expect } from "vitest";
import { navReduce } from "../nav.js";

const V = (o = {}) => ({
  phase: 'position', cursor: 0, pliesLen: 5,
  onlineMode: false, onlineGameOver: false, ...o,
});

describe("navReduce（局面ナビゲーションの純粋遷移）", () => {
  it("prev: reveal → position（cursor 据え置き）", () => {
    expect(navReduce(V({ phase: 'reveal', cursor: 2 }), 'prev')).toEqual({ phase: 'position' });
  });
  it("prev: position(cursor>0) → 前の reveal", () => {
    expect(navReduce(V({ phase: 'position', cursor: 2 }), 'prev')).toEqual({ cursor: 1, phase: 'reveal' });
  });
  it("prev: 初期局面（cursor 0・position）は不可 → null", () => {
    expect(navReduce(V({ phase: 'position', cursor: 0 }), 'prev')).toBeNull();
  });
  it("next: position → reveal", () => {
    expect(navReduce(V({ phase: 'position', cursor: 2 }), 'next')).toEqual({ phase: 'reveal' });
  });
  it("next: reveal → 次の position", () => {
    expect(navReduce(V({ phase: 'reveal', cursor: 2 }), 'next')).toEqual({ cursor: 3, phase: 'position' });
  });
  it("next: 最終局面（cursor===pliesLen・position）は不可 → null", () => {
    expect(navReduce(V({ phase: 'position', cursor: 5, pliesLen: 5 }), 'next')).toBeNull();
  });

  it("オンライン対局中（終局前）はナビ不可 → null（prev/next とも）", () => {
    const v = V({ phase: 'reveal', cursor: 2, onlineMode: true, onlineGameOver: false });
    expect(navReduce(v, 'prev')).toBeNull();
    expect(navReduce(v, 'next')).toBeNull();
  });
  it("オンライン終局後はナビ可", () => {
    const v = V({ phase: 'reveal', cursor: 2, onlineMode: true, onlineGameOver: true });
    expect(navReduce(v, 'prev')).toEqual({ phase: 'position' });
  });

  it("往復の可逆性: next で進んで prev で戻ると元へ", () => {
    let s = { phase: 'position', cursor: 1, pliesLen: 5 };
    const apply = (a) => { const p = navReduce({ ...s, onlineMode: false, onlineGameOver: false }, a); if (p) Object.assign(s, p); };
    apply('next'); // position→reveal
    apply('next'); // reveal→cursor2 position
    expect(s).toMatchObject({ cursor: 2, phase: 'position' });
    apply('prev'); // →cursor1 reveal
    apply('prev'); // reveal→position
    expect(s).toMatchObject({ cursor: 1, phase: 'position' });
  });

  it("各組手は reveal→position の二拍で刻まれる（同時着手の歩み）", () => {
    let s = { phase: 'position', cursor: 0, pliesLen: 3 };
    const trail = [];
    for (let i = 0; i < 6; i++) {
      const p = navReduce({ ...s, onlineMode: false, onlineGameOver: false }, 'next');
      if (!p) break; Object.assign(s, p); trail.push(`${s.cursor}:${s.phase}`);
    }
    expect(trail).toEqual(['0:reveal', '1:position', '1:reveal', '2:position', '2:reveal', '3:position']);
  });

  it("未知の action は null", () => {
    expect(navReduce(V(), 'jump')).toBeNull();
  });
});
