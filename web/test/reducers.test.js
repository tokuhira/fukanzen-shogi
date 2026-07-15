import { describe, it, expect } from "vitest";
import { resetOnlineReduce, hotseatConfirmReduce, turnCompleteDecision, metaToLoadedMeta, archivedLinkFor, endGameReduce } from "../reducers.js";

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

describe("turnCompleteDecision（オンライン組手完了時の投了判断）", () => {
  it("非投了: {kind:'live'}（onlineSide に関わらず）", () => {
    expect(turnCompleteDecision('7g7f', '3c3d', 'sente')).toEqual({ kind: 'live' });
    expect(turnCompleteDecision('7g7f', '3c3d', 'gote')).toEqual({ kind: 'live' });
  });

  it("両者投了: 引き分け（陣営に関わらず同じ）", () => {
    const expected = {
      kind: 'resign', msg: '引き分け（両者投了）', outcome: 'draw',
      resultOverride: { kind: 'resign', outcome: 'draw' },
    };
    expect(turnCompleteDecision('resign', 'resign', 'sente')).toEqual(expected);
    expect(turnCompleteDecision('resign', 'resign', 'gote')).toEqual(expected);
  });

  it("先手投了・自分が先手: 「投了しました（後手の勝ち）」", () => {
    expect(turnCompleteDecision('resign', '7g7f', 'sente')).toEqual({
      kind: 'resign', msg: '投了しました（後手の勝ち）', outcome: 'gote_wins',
      resultOverride: { kind: 'resign', outcome: 'gote_wins' },
    });
  });

  it("先手投了・自分が後手: 「相手が投了しました（先手の勝ち）」", () => {
    expect(turnCompleteDecision('resign', '7g7f', 'gote')).toEqual({
      kind: 'resign', msg: '相手が投了しました（先手の勝ち）', outcome: 'gote_wins',
      resultOverride: { kind: 'resign', outcome: 'gote_wins' },
    });
  });

  it("後手投了・自分が後手: 「投了しました（先手の勝ち）」", () => {
    expect(turnCompleteDecision('7g7f', 'resign', 'gote')).toEqual({
      kind: 'resign', msg: '投了しました（先手の勝ち）', outcome: 'sente_wins',
      resultOverride: { kind: 'resign', outcome: 'sente_wins' },
    });
  });

  it("後手投了・自分が先手: 「相手が投了しました（後手の勝ち）」", () => {
    expect(turnCompleteDecision('7g7f', 'resign', 'sente')).toEqual({
      kind: 'resign', msg: '相手が投了しました（後手の勝ち）', outcome: 'sente_wins',
      resultOverride: { kind: 'resign', outcome: 'sente_wins' },
    });
  });
});

describe("metaToLoadedMeta（版タプル/結果 → loadedMeta）", () => {
  it("version が無ければ null", () => {
    expect(metaToLoadedMeta(null, null)).toBe(null);
    expect(metaToLoadedMeta(undefined, { kind: 'mate', outcome: 'sente_wins' })).toBe(null);
  });

  it("result が無ければ既定値 {kind:'unfinished', outcome:'none'}", () => {
    const version = { rule: '0.6', protocol: 5, app: '0.12.3' };
    expect(metaToLoadedMeta(version, null)).toEqual({
      rule: '0.6', protocol: 5, app: '0.12.3',
      sente: null, gote: null,
      result: { kind: 'unfinished', outcome: 'none' },
    });
  });

  it("result があればそのまま使う", () => {
    const version = { rule: '0.6', protocol: 5, app: '0.12.3' };
    const result = { kind: 'mate', outcome: 'sente_wins' };
    expect(metaToLoadedMeta(version, result)).toEqual({
      rule: '0.6', protocol: 5, app: '0.12.3',
      sente: null, gote: null,
      result,
    });
  });
});

describe("archivedLinkFor（アーカイブ id → リンク情報、archiveUrl 注入）", () => {
  const archiveUrl = (id) => 'u/' + id;

  it("id があれば {id, url}", () => {
    expect(archivedLinkFor('abc', archiveUrl)).toEqual({ id: 'abc', url: 'u/abc' });
  });

  it("id が null/undefined/空文字なら null", () => {
    expect(archivedLinkFor(null, archiveUrl)).toBe(null);
    expect(archivedLinkFor(undefined, archiveUrl)).toBe(null);
    expect(archivedLinkFor('', archiveUrl)).toBe(null);
  });
});

describe("endGameReduce（オンライン対局の終局 patch）", () => {
  it("msg を onlineEndMsg に反映し、固定フィールドを返す", () => {
    expect(endGameReduce('引き分け（両者投了）')).toEqual({
      onlineGameOver: true,
      onlineEndMsg: '引き分け（両者投了）',
      onlineCommitted: false,
      onlineWaiting: false,
    });
  });

  it("任意の msg で onlineEndMsg === msg、他3フィールドは固定", () => {
    expect(endGameReduce('投了しました（後手の勝ち）')).toEqual({
      onlineGameOver: true,
      onlineEndMsg: '投了しました（後手の勝ち）',
      onlineCommitted: false,
      onlineWaiting: false,
    });
  });
});
