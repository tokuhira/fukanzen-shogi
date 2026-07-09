import {
  CELL, BX, BY, BW, BH, SVG_W, SVG_H, PFS, LFS,
  KANJI, HAND_ORDER, RANK_JA, countStr,
} from './geometry.js';

export function renderSvg(pos, overlay) {
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
