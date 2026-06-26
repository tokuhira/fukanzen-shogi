import init, { resolve_ply } from './wasm/engine_wasm.js';

// ─────────────────────────────────────────────────────────────────────────────
// Kifu data — initial SFEN + move list; engine computes resolved positions
// ─────────────────────────────────────────────────────────────────────────────

const INITIAL_SFEN = "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1";

const TURNS = [
  { s:"☗7六歩",  g:"☖3四歩",  sFrom:[7,7], gFrom:[3,3], sTo:[7,6], gTo:[3,4], sDrop:null, gDrop:null, sUsi:"7g7f", gUsi:"3c3d" },
  { s:"☗2六歩",  g:"☖8四歩",  sFrom:[2,7], gFrom:[8,3], sTo:[2,6], gTo:[8,4], sDrop:null, gDrop:null, sUsi:"2g2f", gUsi:"8c8d" },
  { s:"☗2五歩",  g:"☖8五歩",  sFrom:[2,6], gFrom:[8,4], sTo:[2,5], gTo:[8,5], sDrop:null, gDrop:null, sUsi:"2f2e", gUsi:"8d8e" },
  { s:"☗8六歩",  g:"☖8六歩",  sFrom:[8,7], gFrom:[8,5], sTo:[8,6], gTo:[8,6], sDrop:null, gDrop:null, sUsi:"8g8f", gUsi:"8e8f" },
  { s:"☗8六歩打", g:"☖3五歩", sFrom:null,  gFrom:[3,4], sTo:[8,6], gTo:[3,5], sDrop:'P',  gDrop:null, sUsi:"P*8f", gUsi:"3d3e" },
  { s:"☗3三角成", g:"☖8六飛", sFrom:[8,8], gFrom:[8,2], sTo:[3,3], gTo:[8,6], sDrop:null, gDrop:null, sUsi:"8h3c+", gUsi:"8b8f" },
];

// Populated after Wasm init: sfens[i] = SFEN entering turn i
//                            events[i] = engine event string from resolving turn i
let computedSfens  = null;
let computedEvents = null;

// ─────────────────────────────────────────────────────────────────────────────
// Event → display label
// ─────────────────────────────────────────────────────────────────────────────

const EVENT_LABEL = {
  clash:      '相討ち',
  sente_died: '先手玉が取られた',
  gote_died:  '後手玉が取られた',
  both_died:  '両玉相討ち',
};

// ─────────────────────────────────────────────────────────────────────────────
// State
// ─────────────────────────────────────────────────────────────────────────────

let posIndex = 0;
let phase = 'position'; // 'position' | 'reveal'

// ─────────────────────────────────────────────────────────────────────────────
// SFEN parser
// ─────────────────────────────────────────────────────────────────────────────

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
      while (i < handStr.length && handStr[i] >= '0' && handStr[i] <= '9') {
        numStr += handStr[i++];
      }
      const ch = handStr[i++];
      if (!ch) break;
      const count = numStr ? +numStr : 1;
      const side = (ch === ch.toUpperCase()) ? 's' : 'g';
      const kind = ch.toUpperCase();
      if (side === 's') handS[kind] = (handS[kind] || 0) + count;
      else              handG[kind] = (handG[kind] || 0) + count;
    }
  }

  return { board, handS, handG };
}

// ─────────────────────────────────────────────────────────────────────────────
// SVG constants
// ─────────────────────────────────────────────────────────────────────────────

const CELL  = 38;
const BX    = 6;
const BY    = 58;
const BW    = CELL * 9;
const BH    = CELL * 9;
const SVG_W = BX + BW + 30;
const SVG_H = BY + BH + 50;

const PFS = 22;
const LFS = 11;

const KANJI = {
  P:'歩', L:'香', N:'桂', S:'銀', G:'金', B:'角', R:'飛', K:'玉',
  '+P':'と', '+L':'杏', '+N':'圭', '+S':'全', '+B':'馬', '+R':'龍',
};

const HAND_ORDER = ['R','B','G','S','N','L','P'];
const RANK_JA    = ['一','二','三','四','五','六','七','八','九'];

function countStr(n) {
  if (n <= 1) return '';
  return n <= 9 ? RANK_JA[n - 1] : String(n);
}

// ─────────────────────────────────────────────────────────────────────────────
// SVG generation
// ─────────────────────────────────────────────────────────────────────────────

function computeOverlay(t) {
  return {
    board: [t.sFrom, t.sTo, t.gFrom, t.gTo].filter(Boolean),
    sHand: t.sDrop ? new Set([t.sDrop]) : null,
    gHand: t.gDrop ? new Set([t.gDrop]) : null,
  };
}

function renderSvg(pos, overlay) {
  const { board, handS, handG } = pos;
  const buf = [];
  const p = s => buf.push(s);

  p(`<rect width="${SVG_W}" height="${SVG_H}" fill="#f5f3ec"/>`);
  p(`<g font-family="'Noto Serif JP',Georgia,serif">`);

  renderHandArea(buf, handG, '後手持駒', BX, 8, overlay?.gHand, 'g');

  for (let f = 9; f >= 1; f--) {
    const cx = BX + (9 - f) * CELL + CELL / 2;
    p(`<text x="${cx}" y="${BY - 7}" text-anchor="middle" font-size="${LFS}" fill="#777">${f}</text>`);
  }

  p(`<rect x="${BX}" y="${BY}" width="${BW}" height="${BH}" `
  + `fill="none" stroke="#1a1a1a" stroke-width="1.5"/>`);

  for (let i = 1; i < 9; i++) {
    const x = BX + i * CELL;
    p(`<line x1="${x}" y1="${BY}" x2="${x}" y2="${BY + BH}" stroke="#1a1a1a" stroke-width="0.5"/>`);
    const y = BY + i * CELL;
    p(`<line x1="${BX}" y1="${y}" x2="${BX + BW}" y2="${y}" stroke="#1a1a1a" stroke-width="0.5"/>`);
  }

  for (let r = 1; r <= 9; r++) {
    const cy = BY + (r - 1) * CELL + CELL / 2 + 5;
    p(`<text x="${BX + BW + 6}" y="${cy}" font-size="${LFS}" fill="#777">${RANK_JA[r - 1]}</text>`);
  }

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

  for (const [key, piece] of board) {
    const [f, r] = key.split(',').map(Number);
    const kanji = KANJI[piece.kind] || '？';
    const cx    = BX + (9 - f) * CELL + CELL / 2;
    const cy    = BY + (r - 1) * CELL + CELL / 2;
    const dy    = PFS * 0.36;

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
        buf.push(
          `<rect x="${ox - 1}" y="${hly}" width="${txt.length * PFS + 2}" height="${hlh}" `
          + `fill="#1a1a1a" fill-opacity="0.09"/>`
        );
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

// ─────────────────────────────────────────────────────────────────────────────
// Navigation
// ─────────────────────────────────────────────────────────────────────────────

function goNext() {
  if (phase === 'position' && posIndex < TURNS.length) {
    phase = 'reveal';
  } else if (phase === 'reveal') {
    posIndex++;
    phase = 'position';
  }
  render();
}

function goPrev() {
  if (phase === 'reveal') {
    phase = 'position';
  } else if (phase === 'position' && posIndex > 0) {
    posIndex--;
    phase = 'reveal';
  }
  render();
}

// ─────────────────────────────────────────────────────────────────────────────
// Render
// ─────────────────────────────────────────────────────────────────────────────

function render() {
  if (!computedSfens) return;

  const pos     = parseSfen(computedSfens[posIndex]);
  const overlay = phase === 'reveal' ? computeOverlay(TURNS[posIndex]) : null;

  let moveText  = '';
  let phaseText = '';
  let eventText = '';

  if (phase === 'reveal') {
    const t = TURNS[posIndex];
    moveText  = `${t.s}　${t.g}`;
    phaseText = '同時開示';
    const evKey = computedEvents[posIndex];
    eventText = (evKey && evKey !== 'normal') ? `（${EVENT_LABEL[evKey] || evKey}）` : '';
  } else {
    phaseText = posIndex === 0 ? '初期局面' : `第${posIndex}組手後`;
  }

  const svg = document.getElementById('board');
  svg.setAttribute('viewBox', `0 0 ${SVG_W} ${SVG_H}`);
  svg.innerHTML = renderSvg(pos, overlay);

  document.getElementById('phase-label').textContent  = phaseText;
  document.getElementById('move-display').textContent = moveText;
  document.getElementById('event-label').textContent  = eventText || ' ';

  const step  = posIndex * 2 + (phase === 'reveal' ? 1 : 0) + 1;
  const total = (TURNS.length + 1) + TURNS.length;
  document.getElementById('step-label').textContent = `${step} / ${total}`;

  document.getElementById('btn-prev').disabled = (posIndex === 0 && phase === 'position');
  document.getElementById('btn-next').disabled = (posIndex >= TURNS.length && phase === 'position');
}

// ─────────────────────────────────────────────────────────────────────────────
// Wasm init + startup
// ─────────────────────────────────────────────────────────────────────────────

document.addEventListener('DOMContentLoaded', async () => {
  document.getElementById('btn-next').addEventListener('click', goNext);
  document.getElementById('btn-prev').addEventListener('click', goPrev);

  document.addEventListener('keydown', e => {
    if (e.key === 'ArrowRight' || e.key === 'ArrowDown') goNext();
    if (e.key === 'ArrowLeft'  || e.key === 'ArrowUp')   goPrev();
  });

  document.getElementById('phase-label').textContent = '読み込み中…';
  document.getElementById('btn-prev').disabled = true;
  document.getElementById('btn-next').disabled = true;

  try {
    await init();

    const sfens  = [INITIAL_SFEN];
    const events = [];
    for (const t of TURNS) {
      const result = JSON.parse(resolve_ply(sfens[sfens.length - 1], t.sUsi, t.gUsi));
      if (!result.ok) throw new Error(result.error);
      sfens.push(result.sfen);
      events.push(result.event);
    }
    computedSfens  = sfens;
    computedEvents = events;

    render();
  } catch (err) {
    document.getElementById('phase-label').textContent = `読み込みエラー: ${err.message}`;
    console.error(err);
  }
});
