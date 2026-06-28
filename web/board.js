import init, {
  resolve_ply,
  game_status  as wasmGameStatus,
  legal_actions as wasmLegalActions,
} from './wasm/engine_wasm.js';

import {
  connectOnline, disconnectOnline, commitMoveOnline, getMySide,
  reconnectOnline, hasReconnectableSession, debugState,
} from './online.js';

// ── Constants ─────────────────────────────────────────────────────────────────

const INITIAL_SFEN = "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1";

const DEMO_PLIES = [
  { sUsi:"7g7f",  gUsi:"3c3d",  sText:"☗7六歩",   gText:"☖3四歩"  },
  { sUsi:"2g2f",  gUsi:"8c8d",  sText:"☗2六歩",   gText:"☖8四歩"  },
  { sUsi:"2f2e",  gUsi:"8d8e",  sText:"☗2五歩",   gText:"☖8五歩"  },
  { sUsi:"8g8f",  gUsi:"8e8f",  sText:"☗8六歩",   gText:"☖8六歩"  },
  { sUsi:"P*8f",  gUsi:"3d3e",  sText:"☗8六歩打", gText:"☖3五歩"  },
  { sUsi:"8h3c+", gUsi:"8b8f",  sText:"☗3三角成", gText:"☖8六飛"  },
];

const CELL  = 38;
const BX    = 6;
const BY    = 58;
const BW    = CELL * 9;  // 342
const BH    = CELL * 9;  // 342
const SVG_W = BX + BW + 30;  // 378
const SVG_H = BY + BH + 50;  // 450
const PFS   = 22;
const LFS   = 11;

const KANJI = {
  P:'歩', L:'香', N:'桂', S:'銀', G:'金', B:'角', R:'飛', K:'玉',
  '+P':'と', '+L':'杏', '+N':'圭', '+S':'全', '+B':'馬', '+R':'龍',
};
const HAND_ORDER = ['R','B','G','S','N','L','P'];
const RANK_JA    = ['一','二','三','四','五','六','七','八','九'];
const RANK_CHAR  = 'abcdefghi';

const EVENT_LABEL = {
  clash:      '相討ち',
  sente_died: '先手玉が取られた',
  gote_died:  '後手玉が取られた',
  both_died:  '両玉相討ち',
};

// ── USI utilities ─────────────────────────────────────────────────────────────

function charToRank(c) { return RANK_CHAR.indexOf(c) + 1; }

function parseUsi(usi) {
  if (usi[1] === '*') {
    return { usi, isDrop: true, kind: usi[0], to: [parseInt(usi[2]), charToRank(usi[3])], promote: false };
  }
  return {
    usi,
    isDrop:  false,
    from:    [parseInt(usi[0]), charToRank(usi[1])],
    to:      [parseInt(usi[2]), charToRank(usi[3])],
    promote: usi.length === 5,
  };
}

function countStr(n) {
  if (n <= 1) return '';
  return n <= 9 ? RANK_JA[n - 1] : String(n);
}

function usiToText(usi, sfen, side) {
  const prefix = side === 'sente' ? '☗' : '☖';
  const m = parseUsi(usi);
  if (m.isDrop) {
    return `${prefix}${m.to[0]}${RANK_JA[m.to[1] - 1]}${KANJI[m.kind] || m.kind}打`;
  }
  const pos = parseSfen(sfen);
  const piece = pos.board.get(`${m.from[0]},${m.from[1]}`);
  const kanji = piece ? (KANJI[piece.kind] || '？') : '？';
  return `${prefix}${m.to[0]}${RANK_JA[m.to[1] - 1]}${kanji}${m.promote ? '成' : ''}`;
}

// ── SFEN parser ───────────────────────────────────────────────────────────────

function parseSfen(sfen) {
  const parts = sfen.split(' ');
  const boardStr = parts[0];
  const handStr  = parts[2] || '-';

  const board = new Map();
  boardStr.split('/').forEach((row, rankIdx) => {
    const rank = rankIdx + 1;
    let file = 9;
    let i = 0;
    while (i < row.length) {
      const ch = row[i];
      if (ch === '+') {
        const nxt = row[++i];
        const side = (nxt === nxt.toUpperCase()) ? 's' : 'g';
        board.set(`${file},${rank}`, { kind: '+' + nxt.toUpperCase(), side });
        file--;
      } else if (ch >= '1' && ch <= '9') {
        file -= +ch;
      } else {
        const side = (ch === ch.toUpperCase()) ? 's' : 'g';
        board.set(`${file},${rank}`, { kind: ch.toUpperCase(), side });
        file--;
      }
      i++;
    }
  });

  const handS = {}, handG = {};
  if (handStr !== '-') {
    let i = 0;
    while (i < handStr.length) {
      let numStr = '';
      while (i < handStr.length && handStr[i] >= '0' && handStr[i] <= '9') numStr += handStr[i++];
      const ch = handStr[i++];
      if (!ch) break;
      const count = numStr ? +numStr : 1;
      const side  = (ch === ch.toUpperCase()) ? 's' : 'g';
      const kind  = ch.toUpperCase();
      if (side === 's') handS[kind] = (handS[kind] || 0) + count;
      else              handG[kind] = (handG[kind] || 0) + count;
    }
  }

  return { board, handS, handG };
}

// ── Kifu state ────────────────────────────────────────────────────────────────

// plies[i] = { sUsi, gUsi, sText, gText }
const kifu = { plies: [] };
let cursor = 0;
let sfens  = [INITIAL_SFEN];  // sfens[i] = position entering turn i
let events = [];               // events[i] = event string from resolving plies[i]
let phase  = 'position';       // 'position' | 'reveal'

// ── Input state ───────────────────────────────────────────────────────────────

let inputStep        = null;  // null | 'sente' | 'gote'
let pendingSente     = null;  // null | { usi, text }
let pendingGote      = null;  // null | { usi, text }
let selectedFrom     = null;  // null | { board:[f,r] } | { hand:kind }
let legalTargets     = null;  // null | Map<"f,r", { options:[{usi,promote}] }>
let promotionPending = null;  // null | { options, toSquare }

// Per-sfen legal move cache
let legalCache = { sfen: null, sente: null, gote: null };

// Per-cursor game-over cache
let gameOverCache = { cursor: -1, msg: null };

// ── Online mode state ─────────────────────────────────────────────────────────

let onlineMode            = false;
let onlineSide            = null;    // 'sente' | 'gote'
let onlineCommitted       = false;   // 自分の commit 送信済み（解決待ち中）
let onlineGameOver        = false;   // 終局確定（review 中も true を維持）
let onlineEndMsg          = '';      // 終局理由の表示文字列（投了時など）
let onlineWaiting         = false;   // 切断待機中（相手切断 or 自分切断後の再接続待ち）
let onlineWaitingMsg      = '';      // 待機時の表示メッセージ

// ── Kifu management ───────────────────────────────────────────────────────────

function loadPlies(plies) {
  kifu.plies = [];
  sfens  = [INITIAL_SFEN];
  events = [];
  for (const ply of plies) {
    const r = JSON.parse(resolve_ply(sfens.at(-1), ply.sUsi, ply.gUsi));
    if (!r.ok) throw new Error(r.error);
    kifu.plies.push({ ...ply });
    sfens.push(r.sfen);
    events.push(r.event);
  }
  cursor = 0;
  phase  = 'position';
  resetInput();
  gameOverCache = { cursor: -1, msg: null };
}

function resetToNew() {
  kifu.plies = [];
  sfens  = [INITIAL_SFEN];
  events = [];
  cursor = 0;
  phase  = 'position';
  resetInput();
  gameOverCache = { cursor: -1, msg: null };
}

function branchAndAppend(sUsi, gUsi, sText, gText) {
  kifu.plies = kifu.plies.slice(0, cursor);
  sfens  = sfens.slice(0, cursor + 1);
  events = events.slice(0, cursor);
  gameOverCache = { cursor: -1, msg: null };

  const r = JSON.parse(resolve_ply(sfens[cursor], sUsi, gUsi));
  if (!r.ok) throw new Error(r.error);

  kifu.plies.push({ sUsi, gUsi, sText, gText });
  sfens.push(r.sfen);
  events.push(r.event);

  phase = 'reveal';  // cursor stays — reveal shows the move just played
  resetInput();
}

// ── Game-over detection ───────────────────────────────────────────────────────

function getGameOverMsg() {
  if (phase !== 'position') return null;
  if (cursor !== gameOverCache.cursor) {
    gameOverCache = { cursor, msg: computeGameOver() };
  }
  return gameOverCache.msg;
}

function computeGameOver() {
  if (cursor > 0 && cursor === kifu.plies.length) {
    const ev = events[cursor - 1];
    if (ev === 'sente_died') return '後手の勝ち（先手玉が取られた）';
    if (ev === 'gote_died')  return '先手の勝ち（後手玉が取られた）';
    if (ev === 'both_died')  return '引き分け（両玉相討ち）';
  }
  const status = wasmGameStatus(sfens[cursor]);
  if (status === 'sente_loses') return '後手の勝ち（先手が着手不能）';
  if (status === 'gote_loses')  return '先手の勝ち（後手が着手不能）';
  if (status === 'draw')        return '引き分け（両者着手不能）';
  return null;
}

// ── Input management ──────────────────────────────────────────────────────────

function resetInput() {
  inputStep        = null;
  pendingSente     = null;
  pendingGote      = null;
  selectedFrom     = null;
  legalTargets     = null;
  promotionPending = null;
  hidePromotionUI();
}

function getLegalMovesForSide(side) {
  const sfen = sfens[cursor];
  if (legalCache.sfen !== sfen) {
    legalCache = { sfen, sente: null, gote: null };
  }
  if (!legalCache[side]) {
    legalCache[side] = JSON.parse(wasmLegalActions(sfen, side)).map(parseUsi);
  }
  return legalCache[side];
}

function buildTargetMap(moves) {
  const map = new Map();
  for (const m of moves) {
    const key = `${m.to[0]},${m.to[1]}`;
    if (!map.has(key)) map.set(key, { options: [] });
    map.get(key).options.push({ usi: m.usi, promote: m.promote });
  }
  return map;
}

function activateMoves(moves, from) {
  if (!moves.length) { selectedFrom = null; legalTargets = null; return; }
  selectedFrom = from;
  legalTargets = buildTargetMap(moves);
}

function selectBoardPiece(file, rank) {
  if (!inputStep) inputStep = 'sente';
  const side  = inputStep === 'gote' ? 'gote' : 'sente';
  const moves = getLegalMovesForSide(side).filter(
    m => !m.isDrop && m.from[0] === file && m.from[1] === rank
  );
  activateMoves(moves, { board: [file, rank] });
}

function selectHandPiece(kind) {
  if (!inputStep) inputStep = 'sente';
  const side  = inputStep === 'gote' ? 'gote' : 'sente';
  const moves = getLegalMovesForSide(side).filter(
    m => m.isDrop && m.kind === kind.toUpperCase()
  );
  activateMoves(moves, { hand: kind });
}

function selectTarget(file, rank) {
  const key = `${file},${rank}`;
  if (!legalTargets?.has(key)) {
    selectedFrom = null; legalTargets = null; return;
  }
  const { options } = legalTargets.get(key);
  const hasPromote   = options.some(o =>  o.promote);
  const hasNoPromote = options.some(o => !o.promote);
  if (hasPromote && hasNoPromote) {
    promotionPending = { options, toSquare: [file, rank] };
    showPromotionUI();
  } else {
    confirmMove(options[0].usi);
    render();
  }
}

function confirmMove(usi) {
  const side = inputStep === 'gote' ? 'gote' : 'sente';
  const text = usiToText(usi, sfens[cursor], side);

  if (onlineMode) {
    // オンラインモード: 自分の陣営だけ確定して commit を送信する
    if (side === 'sente') pendingSente = { usi, text };
    else                  pendingGote  = { usi, text };
    inputStep        = null;
    selectedFrom     = null;
    legalTargets     = null;
    promotionPending = null;
    hidePromotionUI();
    onlineCommitted  = true;
    commitMoveOnline(sfens[cursor], usi);
    return;
  }

  // ホットシートモード（従来）
  if (side === 'sente') {
    pendingSente = { usi, text };
    inputStep    = 'gote';
  } else {
    pendingGote = { usi, text };
  }
  selectedFrom = null; legalTargets = null;
  promotionPending = null; hidePromotionUI();
}

function _resetOnlineState() {
  onlineMode       = false;
  onlineSide       = null;
  onlineGameOver   = false;
  onlineEndMsg     = '';
  onlineCommitted  = false;
  onlineWaiting    = false;
  onlineWaitingMsg = '';
}

function _onlinePhaseText(gameOver) {
  if (onlineGameOver) {
    if (gameOver || cursor === kifu.plies.length) return onlineEndMsg || gameOver || '終局';
    if (cursor === 0) return '初期局面';
    return `第${cursor}組手後`;
  }
  if (onlineWaiting)   return onlineWaitingMsg;
  if (onlineCommitted) return '着手確定 — 相手の着手を待っています';
  if (onlineSide === 'gote') return selectedFrom ? '後手の手を選択中' : '後手の手を選んでください';
  return selectedFrom ? '先手の手を選択中' : '先手の手を選んでください';
}

function endOnlineGame(msg) {
  onlineGameOver  = true;
  onlineEndMsg    = msg;
  onlineCommitted = false;
  onlineWaiting   = false;
  // 終局後は WS を閉じる（intentional なので onlineMode は破棄しない）
  disconnectOnline();
  render();
}

function handleTurnComplete(senteUsi, goteUsi) {
  onlineCommitted = false;

  // 投了の検出（ルール 5.3 / 5.4）
  const sResign = senteUsi === 'resign';
  const gResign = goteUsi  === 'resign';
  if (sResign || gResign) {
    let msg;
    if (sResign && gResign) {
      msg = '引き分け（両者投了）';
    } else if (sResign) {
      msg = onlineSide === 'sente' ? '投了しました（後手の勝ち）' : '相手が投了しました（先手の勝ち）';
    } else {
      msg = onlineSide === 'gote'  ? '投了しました（先手の勝ち）' : '相手が投了しました（後手の勝ち）';
    }
    endOnlineGame(msg);
    return;
  }

  const sText = usiToText(senteUsi, sfens[cursor], 'sente');
  const gText = usiToText(goteUsi,  sfens[cursor], 'gote');
  branchAndAppend(senteUsi, goteUsi, sText, gText);
  // phase='reveal' のまま待機 → 盤面クリックで次局面へ（handleSvgClick で処理）
  render();
}

// ── Promotion UI ──────────────────────────────────────────────────────────────

function showPromotionUI() {
  document.getElementById('promotion-overlay').classList.add('visible');
}

function hidePromotionUI() {
  document.getElementById('promotion-overlay')?.classList.remove('visible');
}

// ── Click handling ────────────────────────────────────────────────────────────

function svgCoords(event) {
  const svg  = document.getElementById('board');
  const rect = svg.getBoundingClientRect();
  return {
    x: (event.clientX - rect.left) * (SVG_W / rect.width),
    y: (event.clientY - rect.top)  * (SVG_H / rect.height),
  };
}

function getBoardSquare(sx, sy) {
  if (sx < BX || sx >= BX + BW || sy < BY || sy >= BY + BH) return null;
  const file = 9 - Math.floor((sx - BX) / CELL);
  const rank = Math.floor((sy - BY) / CELL) + 1;
  if (file < 1 || file > 9 || rank < 1 || rank > 9) return null;
  return [file, rank];
}

function getHandPieceAt(hand, y0, sx, sy) {
  if (sy < y0 - 4 || sy > y0 + PFS + 4) return null;
  let ox = BX + 74;
  for (const k of HAND_ORDER) {
    const cnt = hand[k] || 0;
    if (cnt <= 0) continue;
    const w = (KANJI[k] + countStr(cnt)).length * PFS;
    if (sx >= ox - 2 && sx <= ox + w + 2) return k;
    ox += w + 4;
  }
  return null;
}

function _advanceFromReveal(sx, sy) {
  cursor++;
  phase = 'position';
  const msg = getGameOverMsg();
  if (msg) { endOnlineGame(msg); return; }

  if (onlineSide === 'gote') inputStep = 'gote';

  // クリック座標が自分の合法手の駒に当たっていれば選択状態へ直接遷移
  const activeSide = onlineSide === 'gote' ? 'g' : 's';
  const pos = parseSfen(sfens[cursor]);
  const sq  = getBoardSquare(sx, sy);
  if (sq) {
    const [f, r] = sq;
    const piece = pos.board.get(`${f},${r}`);
    if (piece && piece.side === activeSide) selectBoardPiece(f, r);
  } else if (onlineSide === 'gote') {
    const k = getHandPieceAt(pos.handG, 8, sx, sy);
    if (k) selectHandPiece(k);
  } else {
    const k = getHandPieceAt(pos.handS, BY + BH + 12, sx, sy);
    if (k) selectHandPiece(k);
  }
  render();
}

function handleSvgClick(event) {
  // 同時開示フェーズ: 盤面・駒台クリックで次局面へ遷移
  if (phase === 'reveal' && onlineMode && !onlineGameOver) {
    const { x: sx, y: sy } = svgCoords(event);
    _advanceFromReveal(sx, sy);
    return;
  }

  if (phase !== 'position') return;
  if (promotionPending)     return;
  if (onlineMode && onlineCommitted) return;

  const { x: sx, y: sy } = svgCoords(event);
  const gameOver = getGameOverMsg();
  const pos      = parseSfen(sfens[cursor]);
  const activeSide = inputStep === 'gote' ? 'g' : 's';

  // If target selection is active, check for legal target click first
  if (legalTargets) {
    const sq = getBoardSquare(sx, sy);
    if (sq) {
      const key = `${sq[0]},${sq[1]}`;
      if (legalTargets.has(key)) { selectTarget(sq[0], sq[1]); render(); return; }
    }
  }

  // Clicks disabled when game is over and no input is active
  if (gameOver && !inputStep) return;

  // Board square click
  const sq = getBoardSquare(sx, sy);
  if (sq) {
    const [f, r] = sq;
    const piece  = pos.board.get(`${f},${r}`);
    if (piece && piece.side === activeSide) {
      // Toggle selection on same piece; switch to different own piece
      if (selectedFrom?.board?.[0] === f && selectedFrom?.board?.[1] === r) {
        selectedFrom = null; legalTargets = null;
      } else {
        selectBoardPiece(f, r);
      }
      render(); return;
    }
    // Clicked empty or opponent square → deselect without changing inputStep
    if (selectedFrom) { selectedFrom = null; legalTargets = null; render(); }
    return;
  }

  // Gote hand (y=8) — only during gote's turn
  if (inputStep === 'gote') {
    const k = getHandPieceAt(pos.handG, 8, sx, sy);
    if (k) {
      if (selectedFrom?.hand === k) { selectedFrom = null; legalTargets = null; }
      else selectHandPiece(k);
      render(); return;
    }
  }

  // Sente hand (y=BY+BH+12) — during sente's turn or before input starts
  if (inputStep !== 'gote') {
    const k = getHandPieceAt(pos.handS, BY + BH + 12, sx, sy);
    if (k) {
      if (selectedFrom?.hand === k) { selectedFrom = null; legalTargets = null; }
      else selectHandPiece(k);
      render(); return;
    }
  }
}

// ── Overlay computation ────────────────────────────────────────────────────────

function computeRevealOverlay() {
  const ply = kifu.plies[cursor];
  if (!ply) return null;
  const s = parseUsi(ply.sUsi);
  const g = parseUsi(ply.gUsi);
  return {
    board:         [s.isDrop ? null : s.from, s.to, g.isDrop ? null : g.from, g.to].filter(Boolean),
    sHand:         s.isDrop ? new Set([s.kind]) : null,
    gHand:         g.isDrop ? new Set([g.kind]) : null,
    legalDots:     null,
    selectedSquare: null,
  };
}

function computeInputOverlay() {
  const overlay = { board: [], sHand: null, gHand: null, legalDots: null, selectedSquare: null };

  if (selectedFrom?.board) {
    overlay.selectedSquare = selectedFrom.board;
  } else if (selectedFrom?.hand) {
    if (inputStep === 'gote') {
      overlay.gHand = overlay.gHand || new Set();
      overlay.gHand.add(selectedFrom.hand);
    } else {
      overlay.sHand = overlay.sHand || new Set();
      overlay.sHand.add(selectedFrom.hand);
    }
  }

  if (legalTargets) {
    overlay.legalDots = new Set(legalTargets.keys());
  }

  return overlay;
}

// ── SVG rendering ─────────────────────────────────────────────────────────────

function renderSvg(pos, overlay) {
  const { board, handS, handG } = pos;
  const buf = [];
  const p   = s => buf.push(s);

  p(`<rect width="${SVG_W}" height="${SVG_H}" fill="#f5f3ec"/>`);
  p(`<g font-family="'Noto Serif JP',Georgia,serif">`);

  renderHandArea(buf, handG, '後手持駒', BX, 8, overlay?.gHand, 'g');

  // Column numbers (9→1 left to right)
  for (let f = 9; f >= 1; f--) {
    const cx = BX + (9 - f) * CELL + CELL / 2;
    p(`<text x="${cx}" y="${BY - 7}" text-anchor="middle" font-size="${LFS}" fill="#777">${f}</text>`);
  }

  p(`<rect x="${BX}" y="${BY}" width="${BW}" height="${BH}" fill="none" stroke="#1a1a1a" stroke-width="1.5"/>`);

  for (let i = 1; i < 9; i++) {
    const x = BX + i * CELL;
    p(`<line x1="${x}" y1="${BY}" x2="${x}" y2="${BY + BH}" stroke="#1a1a1a" stroke-width="0.5"/>`);
    const y = BY + i * CELL;
    p(`<line x1="${BX}" y1="${y}" x2="${BX + BW}" y2="${y}" stroke="#1a1a1a" stroke-width="0.5"/>`);
  }

  // Row labels (一→九top to bottom)
  for (let r = 1; r <= 9; r++) {
    const cy = BY + (r - 1) * CELL + CELL / 2 + 5;
    p(`<text x="${BX + BW + 6}" y="${cy}" font-size="${LFS}" fill="#777">${RANK_JA[r - 1]}</text>`);
  }

  // Move/overlay highlights (pending moves, reveal highlights)
  if (overlay?.board?.length) {
    const done = new Set();
    for (const [f, r] of overlay.board) {
      const key = `${f},${r}`;
      if (done.has(key)) continue;
      done.add(key);
      p(`<rect x="${BX + (9 - f) * CELL}" y="${BY + (r - 1) * CELL}" `
      + `width="${CELL}" height="${CELL}" fill="#1a1a1a" fill-opacity="0.09"/>`);
    }
  }

  // Selected piece (slightly stronger highlight)
  if (overlay?.selectedSquare) {
    const [f, r] = overlay.selectedSquare;
    p(`<rect x="${BX + (9 - f) * CELL}" y="${BY + (r - 1) * CELL}" `
    + `width="${CELL}" height="${CELL}" fill="#1a1a1a" fill-opacity="0.14"/>`);
  }

  // Legal move dots — rendered before pieces so they appear under glyphs
  if (overlay?.legalDots?.size) {
    for (const key of overlay.legalDots) {
      const [f, r] = key.split(',').map(Number);
      const cx = BX + (9 - f) * CELL + CELL / 2;
      const cy = BY + (r - 1) * CELL + CELL / 2;
      p(`<circle cx="${cx}" cy="${cy}" r="5" fill="#1a1a1a" fill-opacity="0.16"/>`);
    }
  }

  // Pieces
  for (const [key, piece] of board) {
    const [f, r] = key.split(',').map(Number);
    const kanji  = KANJI[piece.kind] || '？';
    const cx     = BX + (9 - f) * CELL + CELL / 2;
    const cy     = BY + (r - 1) * CELL + CELL / 2;
    const dy     = PFS * 0.36;
    if (piece.side === 'g') {
      p(`<text transform="rotate(180,${cx},${cy})" x="${cx}" y="${cy + dy}" `
      + `text-anchor="middle" font-size="${PFS}" fill="#1a1a1a">${kanji}</text>`);
    } else {
      p(`<text x="${cx}" y="${cy + dy}" text-anchor="middle" font-size="${PFS}" fill="#1a1a1a">${kanji}</text>`);
    }
  }

  renderHandArea(buf, handS, '先手持駒', BX, BY + BH + 12, overlay?.sHand, 's');

  p('</g>');
  return buf.join('');
}

function renderHandArea(buf, hand, label, x, y, hl = null, side = 's') {
  const pbl = y + PFS;
  const tcy = y + Math.round(PFS * 0.64);
  const lbl = y + Math.round(PFS * 0.64 + LFS * 0.36);
  const hly = y + Math.round(PFS * 0.2);
  const hlh = PFS - 2;

  buf.push(`<text x="${x}" y="${lbl}" font-size="${LFS}" fill="#999">${label}：</text>`);

  const pieces = HAND_ORDER.filter(k => hand[k] > 0);
  let ox = x + 74;

  if (pieces.length === 0) {
    buf.push(`<text x="${ox}" y="${lbl}" font-size="12" fill="#ccc">なし</text>`);
  } else {
    for (const k of pieces) {
      const txt = KANJI[k] + countStr(hand[k]);
      if (hl && hl.has(k)) {
        buf.push(`<rect x="${ox - 1}" y="${hly}" width="${txt.length * PFS + 2}" height="${hlh}" `
        + `fill="#1a1a1a" fill-opacity="0.09"/>`);
      }
      if (side === 'g') {
        const tcx = ox + Math.round(txt.length * PFS / 2);
        buf.push(`<text transform="rotate(180,${tcx},${tcy})" x="${tcx}" y="${pbl}" `
        + `text-anchor="middle" font-size="${PFS}" fill="#1a1a1a">${txt}</text>`);
      } else {
        buf.push(`<text x="${ox}" y="${pbl}" font-size="${PFS}" fill="#1a1a1a">${txt}</text>`);
      }
      ox += txt.length * PFS + 4;
    }
  }
}

// ── Navigation ────────────────────────────────────────────────────────────────

function goNext() {
  if (promotionPending) return;
  if (onlineMode && !onlineGameOver) return; // 対局中はナビ不可

  if (pendingSente && pendingGote) {
    branchAndAppend(pendingSente.usi, pendingGote.usi, pendingSente.text, pendingGote.text);
    render(); return;
  }

  if (phase === 'position' && cursor < kifu.plies.length) {
    phase = 'reveal';
  } else if (phase === 'reveal') {
    cursor++;
    phase = 'position';
  }
  render();
}

function goPrev() {
  if (onlineMode && !onlineGameOver) return; // 対局中はナビ不可
  if (promotionPending) {
    promotionPending = null; hidePromotionUI();
    selectedFrom = null; legalTargets = null;
    render(); return;
  }

  if (inputStep !== null || selectedFrom !== null) {
    // One press cancels all input state; second press starts navigating back
    resetInput();
    render(); return;
  }

  if (phase === 'reveal') {
    phase = 'position';
  } else if (phase === 'position' && cursor > 0) {
    cursor--;
    phase = 'reveal';
  }
  render();
}

// ── Render ────────────────────────────────────────────────────────────────────

function render() {
  const pos       = parseSfen(sfens[cursor]);
  const bothReady = !!(pendingSente && pendingGote);
  const hasInput  = !!(inputStep || selectedFrom || pendingSente || pendingGote);
  const gameOver  = getGameOverMsg();

  let overlay, moveText = '', phaseText = '', eventText = '';

  if (phase === 'reveal') {
    overlay   = computeRevealOverlay();
    const ply = kifu.plies[cursor];
    moveText  = `${ply.sText}　${ply.gText}`;
    phaseText = '同時開示';
    const evKey = events[cursor];
    eventText = (evKey && evKey !== 'normal') ? `（${EVENT_LABEL[evKey] || evKey}）` : '';
  } else {
    overlay = hasInput ? computeInputOverlay() : null;

    if (onlineMode) {
      phaseText = _onlinePhaseText(gameOver);
      if (!onlineGameOver && onlineCommitted) {
        moveText = onlineSide === 'sente' ? (pendingSente?.text || '') : (pendingGote?.text || '');
      }
    } else if (bothReady) {
      moveText  = `${pendingSente.text}　${pendingGote.text}`;
      phaseText = '解決してください';
    } else if (pendingSente) {
      moveText  = pendingSente.text;
      phaseText = '後手の手を選択中';
    } else if (inputStep === 'gote') {
      phaseText = '後手の手を選択中';
    } else if (inputStep === 'sente' || selectedFrom) {
      phaseText = '先手の手を選択中';
    } else if (gameOver) {
      phaseText = gameOver;
    } else if (cursor === 0) {
      phaseText = '初期局面';
    } else {
      phaseText = `第${cursor}組手後`;
    }
  }

  const svg = document.getElementById('board');
  svg.setAttribute('viewBox', `0 0 ${SVG_W} ${SVG_H}`);
  svg.innerHTML = renderSvg(pos, overlay);
  svg.style.cursor = (phase === 'position' && !gameOver && !(onlineMode && onlineCommitted))
    ? 'pointer' : 'default';

  document.getElementById('phase-label').textContent  = phaseText;
  document.getElementById('move-display').textContent = moveText;
  document.getElementById('event-label').textContent  = eventText || ' ';

  const total = kifu.plies.length * 2 + 1;
  const step  = cursor * 2 + (phase === 'reveal' ? 1 : 0) + 1;
  document.getElementById('step-label').textContent = `${step} / ${total}`;

  const btnNext = document.getElementById('btn-next');
  const btnPrev = document.getElementById('btn-prev');

  if (onlineMode) {
    btnNext.textContent = '次 →';
    if (onlineGameOver) {
      // 終局後は棋譜ナビゲーションを解放（phase に関係なく維持）
      btnNext.disabled = !(
        phase === 'reveal' ||
        (phase === 'position' && cursor < kifu.plies.length)
      );
      btnPrev.disabled = cursor === 0 && phase === 'position';
    } else {
      btnNext.disabled = true;
      btnPrev.disabled = true;
    }
  } else {
    btnNext.textContent = bothReady ? '解決 →' : '次 →';
    btnNext.disabled    = !(
      bothReady ||
      phase === 'reveal' ||
      (phase === 'position' && !hasInput && cursor < kifu.plies.length)
    );
    btnPrev.disabled    = (
      cursor === 0 && phase === 'position' && !hasInput && !promotionPending
    );
  }

  const btnResign = document.getElementById('btn-resign');
  if (btnResign) {
    btnResign.style.display = (onlineMode && !onlineGameOver) ? 'inline-block' : 'none';
    btnResign.disabled      = onlineCommitted || onlineWaiting;
  }
}

// ── Init ──────────────────────────────────────────────────────────────────────

// Escape キーと閉じるボタンの両方から呼べるようモジュールスコープに置く
let closeModal = () => {};

document.addEventListener('DOMContentLoaded', async () => {
  window.__fukanzenDebug = () => console.table(debugState());
  document.getElementById('board').addEventListener('click', handleSvgClick);
  document.getElementById('btn-next').addEventListener('click', goNext);
  document.getElementById('btn-prev').addEventListener('click', goPrev);
  document.getElementById('btn-demo').addEventListener('click', () => { loadPlies(DEMO_PLIES); render(); });
  document.getElementById('btn-new').addEventListener('click', () => {
    if (onlineGameOver) _resetOnlineState();
    resetToNew();
    render();
  });

  document.getElementById('btn-promote').addEventListener('click', () => {
    if (!promotionPending) return;
    const usi = promotionPending.options.find(o => o.promote)?.usi;
    if (usi) { confirmMove(usi); render(); }
  });
  document.getElementById('btn-no-promote').addEventListener('click', () => {
    if (!promotionPending) return;
    const usi = promotionPending.options.find(o => !o.promote)?.usi;
    if (usi) { confirmMove(usi); render(); }
  });

  document.addEventListener('keydown', e => {
    if (e.key === 'ArrowRight' || e.key === 'ArrowDown') goNext();
    if (e.key === 'ArrowLeft'  || e.key === 'ArrowUp')   goPrev();
    if (e.key === 'Escape') {
      const modal = document.getElementById('online-modal');
      if (modal.classList.contains('visible')) {
        closeModal();
      } else {
        resetInput(); render();
      }
    }
  });

  // ── オンライン対戦モーダル ──────────────────────────────────────────────────
  {
    const modal     = document.getElementById('online-modal');
    const statusEl  = document.getElementById('online-status');
    const btnConn   = document.getElementById('btn-connect');
    const btnClose  = document.getElementById('btn-online-close');

    closeModal = () => {
      if (!onlineGameOver) {
        disconnectOnline();
        if (onlineMode) { _resetOnlineState(); resetToNew(); }
      }
      modal.classList.remove('visible');
      statusEl.textContent = '—';
      btnConn.disabled = false;
      btnConn.textContent = '入室';
      render();
    };

    document.getElementById('btn-resign').addEventListener('click', () => {
      if (!onlineMode || onlineGameOver || onlineCommitted) return;
      if (!confirm('投了しますか？')) return;
      // 投了は commit-reveal プロトコル経由。即終局にしない（両者投了の引き分けを拾うため）
      commitMoveOnline(sfens[cursor], 'resign');
      onlineCommitted = true;
      render();
    });

    document.getElementById('btn-online').addEventListener('click', () => {
      modal.classList.add('visible');
      document.getElementById('input-room').focus();
    });

    btnClose.addEventListener('click', closeModal);

    btnConn.addEventListener('click', async () => {
      // 「再接続」ボタンとして機能する場合（self_disconnected 後）
      if (btnConn.textContent === '再接続' && hasReconnectableSession()) {
        btnConn.disabled = true;
        statusEl.textContent = '再接続中…';
        await reconnectOnline();
        return;
      }

      const roomKey = document.getElementById('input-room').value.trim();
      const secret  = document.getElementById('input-secret').value;
      if (!roomKey) {
        statusEl.textContent = 'ルームキーを入力してください';
        return;
      }
      btnConn.disabled = true;
      statusEl.textContent = '接続中…';

      const callbacks = {
        onStatus: (state, msg) => {
          statusEl.textContent = msg;

          if (state === 'ready') {
            if (!onlineMode) {
              // 初回接続: オンラインモード開始
              onlineMode       = true;
              onlineSide       = getMySide();
              onlineCommitted  = false;
              onlineGameOver   = false;
              onlineEndMsg     = '';
              onlineWaiting    = false;
              onlineWaitingMsg = '';
              resetToNew();
              if (onlineSide === 'gote') inputStep = 'gote';
            } else {
              // 再接続完了: ゲーム状態はそのまま、waiting 解除
              onlineWaiting    = false;
              onlineWaitingMsg = '';
              onlineCommitted  = false;
            }
            modal.classList.remove('visible');
            btnConn.disabled = false;
            btnConn.textContent = '入室';
            render();

          } else if (state === 'peer_disconnected') {
            // 相手が切断: ゲーム状態維持、待機表示
            onlineWaiting    = true;
            onlineWaitingMsg = msg;
            onlineCommitted  = false;
            render();

          } else if (state === 'self_disconnected') {
            // 自分が切断: 再接続可能な状態で待機
            onlineWaiting    = true;
            onlineWaitingMsg = msg;
            onlineCommitted  = false;
            btnConn.disabled = false;
            btnConn.textContent = '再接続';
            render();

          } else if (state === 'error') {
            if (onlineMode && !onlineGameOver) {
              onlineWaiting    = true;
              onlineWaitingMsg = `エラー: ${msg}`;
            }
            btnConn.disabled = false;
            btnConn.textContent = '入室';
            render();

          } else if (state === 'disconnected') {
            if (!onlineGameOver) _resetOnlineState();
            btnConn.disabled = false;
            btnConn.textContent = '入室';
            render();
          }
        },
        onTurnComplete:  handleTurnComplete,
        onPeerAborted:   (reason) => endOnlineGame(`中断: ${reason}`),
        getSfens:        () => sfens,
        onResumeAt:      (resumeSfen) => {
          const idx = sfens.indexOf(resumeSfen);
          if (idx >= 0) {
            cursor           = idx;
            phase            = 'position';
            onlineWaiting    = false;
            onlineWaitingMsg = '';
            onlineCommitted  = false;
            resetInput();  // selectedFrom・legalTargets 等をクリア（inputStep は null になる）
            inputStep = onlineSide === 'gote' ? 'gote' : 'sente';
          }
          render();
        },
      };

      await connectOnline(roomKey, secret, callbacks);
    });
  }

  document.getElementById('phase-label').textContent = '読み込み中…';
  document.getElementById('btn-prev').disabled = true;
  document.getElementById('btn-next').disabled = true;

  try {
    await init();
    resetToNew();
    render();
  } catch (err) {
    document.getElementById('phase-label').textContent = `読み込みエラー: ${err.message}`;
    console.error(err);
  }
});
