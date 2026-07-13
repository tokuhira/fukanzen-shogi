import { describe, it, expect } from "vitest";
import { labelView, watchPhaseText, onlinePhaseText, archiveInfoText } from "../view-model.js";

// labelView が参照しうる全フィールドを持つ最小の base state。
const S = (o = {}) => ({
  phase: 'position',
  cursor: 0,
  plies: [],
  events: [],
  pendingSente: null,
  pendingGote: null,
  watchMode: false,
  watchStatusText: '',
  onlineMode: false,
  onlineSide: null,
  onlineGameOver: false,
  onlineEndMsg: '',
  onlineWaiting: false,
  onlineWaitingMsg: '',
  onlineCommitted: false,
  inputStep: null,
  selectedFrom: null,
  loadedMeta: null,
  versionTuple: null,
  ...o,
});

describe("labelView — reveal 局面", () => {
  it("event なし（normal）: eventText は空", () => {
    const s = S({
      phase: 'reveal', cursor: 0,
      plies: [{ sText: '☗７六歩', gText: '☖３四歩' }],
      events: ['normal'],
    });
    const vm = labelView(s, null);
    expect(vm.phaseText).toBe('同時開示');
    expect(vm.moveText).toBe('☗７六歩　☖３四歩');
    expect(vm.eventText).toBe('');
  });

  it("event あり: 括弧付きの日本語ラベル", () => {
    const s = S({
      phase: 'reveal', cursor: 0,
      plies: [{ sText: '☗５五角', gText: '☖５五角' }],
      events: ['both_died'],
    });
    const vm = labelView(s, null);
    expect(vm.eventText).toBe('（両玉相討ち）');
  });

  it("未知の event キーはキー自体をそのまま括弧に入れる", () => {
    const s = S({
      phase: 'reveal', cursor: 0,
      plies: [{ sText: 'a', gText: 'b' }],
      events: ['unknown_key'],
    });
    expect(labelView(s, null).eventText).toBe('（unknown_key）');
  });
});

describe("watchPhaseText / labelView（観戦モード）", () => {
  it("connecting/error/closed", () => {
    expect(watchPhaseText(S({ watchStatusText: 'connecting' }), null)).toBe('観戦: 接続中…');
    expect(watchPhaseText(S({ watchStatusText: 'error' }), null)).toBe('観戦: 接続エラーが発生しました');
    expect(watchPhaseText(S({ watchStatusText: 'closed' }), null)).toBe('観戦: 接続が切れました');
  });

  it("player_disconnected（未終局）は再接続待ち表示", () => {
    const s = S({ watchStatusText: 'player_disconnected' });
    expect(watchPhaseText(s, null)).toBe('観戦: プレイヤーが切断中です（再接続を待っています）');
  });

  it("player_disconnected でも終局済み（concluded）なら結果表示を優先", () => {
    const s = S({
      watchStatusText: 'player_disconnected',
      loadedMeta: { result: { kind: 'resign', outcome: 'sente_wins' } },
      plies: [{ sText: 'a', gText: 'b' }],
      cursor: 1,
    });
    expect(watchPhaseText(s, null)).toBe('先手の勝ち（投了）');
  });

  it("gameOver 注入があれば優先表示", () => {
    expect(watchPhaseText(S({}), '先手の勝ち（後手玉が取られた）')).toBe('先手の勝ち（後手玉が取られた）');
  });

  it("組手が空: 開始待ち", () => {
    expect(watchPhaseText(S({ plies: [] }), null)).toBe('観戦中（開始を待っています）');
  });

  it("最新局面: 観戦中（最新）", () => {
    const s = S({ plies: [{ sText: 'a', gText: 'b' }], cursor: 1 });
    expect(watchPhaseText(s, null)).toBe('観戦中（最新）');
  });

  it("途中局面: 第N組手", () => {
    const s = S({ plies: [{ sText: 'a', gText: 'b' }, { sText: 'c', gText: 'd' }], cursor: 1 });
    expect(watchPhaseText(s, null)).toBe('観戦中（第1組手）');
  });

  it("labelView 経由でも watchMode 分岐が使われる", () => {
    const s = S({ watchMode: true, watchStatusText: 'connecting' });
    expect(labelView(s, null).phaseText).toBe('観戦: 接続中…');
  });
});

describe("onlinePhaseText / labelView（オンライン対局）", () => {
  it("waiting はメッセージそのまま", () => {
    const s = S({ onlineWaiting: true, onlineWaitingMsg: '相手を待っています…' });
    expect(onlinePhaseText(s, null)).toBe('相手を待っています…');
  });

  it("committed（着手確定・相手待ち）", () => {
    expect(onlinePhaseText(S({ onlineCommitted: true }), null)).toBe('着手確定 — 相手の着手を待っています');
  });

  it("labelView: committed 時の moveText は自陣営の pending を表示", () => {
    const s = S({
      onlineMode: true, onlineSide: 'sente', onlineCommitted: true,
      pendingSente: { text: '☗７六歩', usi: '7g7f' },
    });
    const vm = labelView(s, null);
    expect(vm.phaseText).toBe('着手確定 — 相手の着手を待っています');
    expect(vm.moveText).toBe('☗７六歩');
  });

  it("後手番・未選択/選択中", () => {
    expect(onlinePhaseText(S({ onlineSide: 'gote' }), null)).toBe('後手の手を選んでください');
    expect(onlinePhaseText(S({ onlineSide: 'gote', selectedFrom: [3, 3] }), null)).toBe('後手の手を選択中');
  });

  it("先手番・未選択/選択中", () => {
    expect(onlinePhaseText(S({ onlineSide: 'sente' }), null)).toBe('先手の手を選んでください');
    expect(onlinePhaseText(S({ onlineSide: 'sente', selectedFrom: [7, 7] }), null)).toBe('先手の手を選択中');
  });

  it("終局: onlineEndMsg 優先、なければ gameOver、それも無ければ「終局」", () => {
    const base = { onlineGameOver: true, plies: [{ sText: 'a', gText: 'b' }], cursor: 1 };
    expect(onlinePhaseText(S({ ...base, onlineEndMsg: '先手の勝ち' }), null)).toBe('先手の勝ち');
    expect(onlinePhaseText(S({ ...base, onlineEndMsg: '' }), '引き分け')).toBe('引き分け');
    // cursor !== plies.length かつ gameOver も無いときだけ cursor===0/途中の分岐に落ちる
    const midBase = { onlineGameOver: true, plies: [{ sText: 'a', gText: 'b' }, { sText: 'c', gText: 'd' }] };
    expect(onlinePhaseText(S({ ...midBase, onlineEndMsg: '', cursor: 0 }), null)).toBe('初期局面');
  });

  it("終局後・途中局面: 第N組手後", () => {
    const s = S({
      onlineGameOver: true, cursor: 1,
      plies: [{ sText: 'a', gText: 'b' }, { sText: 'c', gText: 'd' }],
    });
    expect(onlinePhaseText(s, null)).toBe('第1組手後');
  });
});

describe("labelView（ローカル・ホットシート）", () => {
  it("bothReady: 「解決してください」＋両者の手", () => {
    const s = S({
      pendingSente: { text: '☗７六歩' },
      pendingGote: { text: '☖３四歩' },
    });
    const vm = labelView(s, null);
    expect(vm.phaseText).toBe('解決してください');
    expect(vm.moveText).toBe('☗７六歩　☖３四歩');
  });

  it("先手だけ確定済み: 後手の手を選択中", () => {
    const s = S({ pendingSente: { text: '☗７六歩' } });
    const vm = labelView(s, null);
    expect(vm.phaseText).toBe('後手の手を選択中');
    expect(vm.moveText).toBe('☗７六歩');
  });

  it("inputStep gote / sente", () => {
    expect(labelView(S({ inputStep: 'gote' }), null).phaseText).toBe('後手の手を選択中');
    expect(labelView(S({ inputStep: 'sente' }), null).phaseText).toBe('先手の手を選択中');
  });

  it("selectedFrom のみでも先手の手を選択中", () => {
    expect(labelView(S({ selectedFrom: [5, 5] }), null).phaseText).toBe('先手の手を選択中');
  });

  it("gameOver 注入があれば表示", () => {
    expect(labelView(S({}), '後手の勝ち（先手玉が取られた）').phaseText).toBe('後手の勝ち（先手玉が取られた）');
  });

  it("初期局面（cursor 0）", () => {
    expect(labelView(S({ cursor: 0 }), null).phaseText).toBe('初期局面');
  });

  it("途中局面: 第N組手後", () => {
    const s = S({ cursor: 3, plies: [1, 2, 3, 4].map(() => ({ sText: 'a', gText: 'b' })) });
    expect(labelView(s, null).phaseText).toBe('第3組手後');
  });
});

describe("archiveInfoText", () => {
  it("loadedMeta が無ければ空", () => {
    expect(archiveInfoText(S({}))).toEqual({ text: '', mismatch: false });
  });

  it("app あり・版一致: version行 — result行", () => {
    const s = S({
      versionTuple: { rule: '0.6' },
      loadedMeta: { rule: '0.6', protocol: 5, app: '0.12.3', result: { kind: 'resign', outcome: 'sente_wins' } },
    });
    expect(archiveInfoText(s)).toEqual({
      text: 'ルール 0.6 / プロトコル 5 / v0.12.3 — 先手の勝ち（投了）',
      mismatch: false,
    });
  });

  it("app 無し: version行に v がつかない", () => {
    const s = S({
      loadedMeta: { rule: '0.6', protocol: 5, app: null, result: { kind: 'unfinished', outcome: 'none' } },
    });
    expect(archiveInfoText(s).text).toBe('ルール 0.6 / プロトコル 5 — 未完');
  });

  it("ルール不一致: 警告文を追記し mismatch=true", () => {
    const s = S({
      versionTuple: { rule: '0.6' },
      loadedMeta: { rule: '0.5', protocol: 4, app: '0.7.0', result: { kind: 'unfinished', outcome: 'none' } },
    });
    const info = archiveInfoText(s);
    expect(info.mismatch).toBe(true);
    expect(info.text).toContain('この棋譜はルール 0.5 で指されました');
  });

  it("labelView は archiveInfo をそのまま含む", () => {
    const s = S({
      loadedMeta: { rule: '0.6', protocol: 5, app: '0.12.3', result: { kind: 'unfinished', outcome: 'none' } },
    });
    expect(labelView(s, null).archiveInfo.text).toContain('未完');
  });
});

describe("labelView — step / total", () => {
  it("position 局面: step は cursor*2+1", () => {
    const s = S({ phase: 'position', cursor: 2, plies: [1, 2, 3].map(() => ({ sText: 'a', gText: 'b' })) });
    const vm = labelView(s, null);
    expect(vm.step).toBe(5);
    expect(vm.total).toBe(7);
  });

  it("reveal 局面: position より 1 大きい", () => {
    const s = S({
      phase: 'reveal', cursor: 2, events: ['normal', 'normal', 'normal'],
      plies: [1, 2, 3].map(() => ({ sText: 'a', gText: 'b' })),
    });
    expect(labelView(s, null).step).toBe(6);
  });
});
