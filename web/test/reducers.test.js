import { describe, it, expect } from "vitest";
import { resetOnlineReduce, hotseatConfirmReduce } from "../reducers.js";

describe("resetOnlineReduce（オンライン状態の初期化）", () => {
  it("11 のオンライン関連キーをすべて初期値へ戻す", () => {
    const p = resetOnlineReduce();
    expect(p).toEqual({
      onlineMode: false, onlineSide: null, onlineGameOver: false, onlineEndMsg: '',
      onlineCommitted: false, onlineWaiting: false, onlineWaitingMsg: '',
      resultOverride: null, recordInviteAsked: false, recordStatusText: '',
      archivedLink: null, _pendingRecordDisconnect: false,
    });
  });
  it("呼ぶたびに独立した新しいオブジェクトを返す（共有しない）", () => {
    expect(resetOnlineReduce()).not.toBe(resetOnlineReduce());
  });
});

describe("hotseatConfirmReduce（ホットシート確定の遷移）", () => {
  it("先手確定：pendingSente をセットし後手入力へ進む＋選択/成りクリア", () => {
    const p = hotseatConfirmReduce('sente', { usi: '7g7f', text: '☗７六歩' });
    expect(p).toEqual({
      pendingSente: { usi: '7g7f', text: '☗７六歩' },
      inputStep: 'gote', selectedFrom: null, legalTargets: null, promotionPending: null,
    });
  });
  it("後手確定：pendingGote をセット（inputStep は進めない）＋選択/成りクリア", () => {
    const p = hotseatConfirmReduce('gote', { usi: '3c3d', text: '☖３四歩' });
    expect(p).toEqual({
      pendingGote: { usi: '3c3d', text: '☖３四歩' },
      selectedFrom: null, legalTargets: null, promotionPending: null,
    });
    expect('inputStep' in p).toBe(false);  // 後手確定は inputStep を触らない
  });
});
