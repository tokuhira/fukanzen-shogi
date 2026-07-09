import { describe, it, expect } from "vitest";
import { formatResult, terminalMessageJa } from "../result-view.js";

describe("formatResult", () => {
  it("outcome 有りは「勝敗（種別）」", () => {
    expect(formatResult({ kind: "mate", outcome: "gote_wins" })).toBe("後手の勝ち（詰み）");
  });

  it("outcome が none なら括弧なし（投了・未完など）", () => {
    expect(formatResult({ kind: "resign", outcome: "none" })).toBe("投了");
    expect(formatResult({ kind: "unfinished", outcome: "none" })).toBe("未完");
  });
});

describe("terminalMessageJa", () => {
  it("mate の全 outcome", () => {
    expect(terminalMessageJa("mate", "gote_wins", 500)).toBe("後手の勝ち（先手が着手不能）");
    expect(terminalMessageJa("mate", "sente_wins", 500)).toBe("先手の勝ち（後手が着手不能）");
    expect(terminalMessageJa("mate", "draw", 500)).toBe("引き分け（両者着手不能）");
  });

  it("king_death の両 outcome", () => {
    expect(terminalMessageJa("king_death", "gote_wins", 500)).toBe("後手の勝ち（先手玉が取られた）");
    expect(terminalMessageJa("king_death", "sente_wins", 500)).toBe("先手の勝ち（後手玉が取られた）");
  });

  it("swap_draw / sennichite", () => {
    expect(terminalMessageJa("swap_draw", "draw", 500)).toBe("引き分け（両玉相討ち）");
    expect(terminalMessageJa("sennichite", "draw", 500)).toBe("引き分け（千日手）");
  });

  it("max_turns は maxTurns 引数を文中へ埋め込む（グローバル非依存の確認）", () => {
    expect(terminalMessageJa("max_turns", "draw", 500)).toBe("引き分け（最長手数・500組手）");
    expect(terminalMessageJa("max_turns", "draw", 123)).toBe("引き分け（最長手数・123組手）");
  });

  it("該当しない組み合わせは null", () => {
    expect(terminalMessageJa("mate", "unknown", 500)).toBeNull();
    expect(terminalMessageJa("unknown", "draw", 500)).toBeNull();
  });
});
