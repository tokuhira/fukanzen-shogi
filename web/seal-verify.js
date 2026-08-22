// 封蝋の検証（記録係三段目 Seal-4）。純粋——DOM・可変状態・fetch に非依存。
// crypto は既定で globalThis.crypto.subtle（ブラウザと node の双方に在る）を使い、
// テストからは差し替えられる（unsupported 経路を試すため）。

export const SEAL_MAGIC = 'fukanzen-shogi-seal';
export const SEAL_FORMAT_VERSION = 1;

export function fromBase64Url(s) {
  const b64 = s.replace(/-/g, '+').replace(/_/g, '/');
  const pad = b64.length % 4 === 0 ? '' : '='.repeat(4 - (b64.length % 4));
  const bin = atob(b64 + pad);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

async function sha256Hex(text, subtle) {
  const d = await subtle.digest('SHA-256', new TextEncoder().encode(text));
  return Array.from(new Uint8Array(d)).map(b => b.toString(16).padStart(2, '0')).join('');
}

// 行ベース statement を { version, kind, id, ... } へ。壊れていれば null。
export function parseStatement(statement) {
  if (typeof statement !== 'string') return null;
  const lines = statement.split('\n');
  const head = lines[0]?.split(' ');
  if (!head || head[0] !== SEAL_MAGIC) return null;
  const version = Number(head[1]);
  if (!Number.isInteger(version)) return null;
  const fields = { version };
  for (const line of lines.slice(1)) {
    const i = line.indexOf(' ');
    if (i < 0) return null;
    fields[line.slice(0, i)] = line.slice(i + 1);
  }
  return fields;
}

export async function sealVerdict(id, envelope, keys, subtle = globalThis.crypto?.subtle) {
  const seal = envelope?.seal;
  if (!seal) return 'unsealed';
  if (!subtle) return 'unsupported';
  if (seal.alg !== 'Ed25519') return 'unsupported';

  const st = parseStatement(seal.statement);
  if (!st) return 'tampered';
  if (st.version !== SEAL_FORMAT_VERSION) return 'unsupported';  // 未来の書式は読めない

  const key = (keys || []).find(k => k && k.kid === seal.kid);
  if (!key) return 'unknown_key';
  if (st.kid !== seal.kid) return 'tampered';
  if (st.id !== id) return 'tampered';
  if (st.archived_at !== envelope.archived_at) return 'tampered';

  if (envelope.finalized === true) {
    if (st.kind !== 'finalized') return 'tampered';
    if (st.witnesses !== String(envelope.witnesses)) return 'tampered';
    if (st.id !== await sha256Hex(envelope.text, subtle)) return 'tampered';
  } else if (envelope.disputed === true) {
    if (st.kind !== 'disputed') return 'tampered';
    const [a, b] = envelope.texts || [];
    if (st.text_a !== await sha256Hex(a ?? '', subtle)) return 'tampered';
    if (st.text_b !== await sha256Hex(b ?? '', subtle)) return 'tampered';
  } else {
    return 'tampered';
  }

  // 三つの失敗を混同しないため、デコードと importKey を別々に受ける。
  // 鍵材が壊れている（公開口の問題）／環境が Ed25519 を扱えない／
  // 署名が壊れている（記録の問題）は、それぞれ別の知らせ。
  let rawPub;
  try { rawPub = fromBase64Url(key.public_key); } catch { return 'unknown_key'; }
  let sigBytes;
  try { sigBytes = fromBase64Url(seal.sig); } catch { return 'tampered'; }

  let pub;
  try {
    pub = await subtle.importKey('raw', rawPub, { name: 'Ed25519' }, false, ['verify']);
  } catch {
    return 'unsupported';   // この環境は Ed25519 を扱えない
  }
  try {
    const ok = await subtle.verify({ name: 'Ed25519' }, pub, sigBytes,
      new TextEncoder().encode(seal.statement));
    return ok ? 'verified' : 'tampered';
  } catch {
    return 'tampered';
  }
}
