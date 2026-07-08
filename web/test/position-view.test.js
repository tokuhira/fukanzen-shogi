import { describe, it, expect } from "vitest";
import { positionViewToState } from "../position-view.js";

// position_view（engine-wasm）が返す JSON view を手書きし、Wasm を経由せず
// アダプタの組み替えだけを検証する（board.js 分割 第〇段）。

describe("positionViewToState", () => {
  it("board 配列を file,rank キーの Map へ組み替える", () => {
    const view = {
      board: [
        { file: 2, rank: 8, kind: "R", side: "s" },
        { file: 8, rank: 8, kind: "B", side: "s" },
        { file: 5, rank: 3, kind: "+P", side: "g" },
      ],
      hand_s: {},
      hand_g: {},
    };
    const state = positionViewToState(view);
    expect(state.board.get("2,8")).toEqual({ kind: "R", side: "s" });
    expect(state.board.get("8,8")).toEqual({ kind: "B", side: "s" });
    expect(state.board.get("5,3")).toEqual({ kind: "+P", side: "g" });
    expect(state.board.size).toBe(3);
  });

  it("hand_s/hand_g をそのまま handS/handG へ渡す", () => {
    const view = {
      board: [],
      hand_s: { P: 2, B: 1 },
      hand_g: { P: 1 },
    };
    const state = positionViewToState(view);
    expect(state.handS).toEqual({ P: 2, B: 1 });
    expect(state.handG).toEqual({ P: 1 });
  });

  it("hand_s/hand_g が欠けている場合は空オブジェクトにする", () => {
    const state = positionViewToState({ board: [] });
    expect(state.handS).toEqual({});
    expect(state.handG).toEqual({});
  });

  it("空局面では空の Map を返す", () => {
    const state = positionViewToState({ board: [], hand_s: {}, hand_g: {} });
    expect(state.board.size).toBe(0);
  });
});
