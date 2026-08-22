import { describe, it, expect, beforeAll } from "vitest";
import { sealVerdict, parseStatement } from "../seal-verify.js";

// 封蝋の検証（記録係三段目 Seal-4 §6）。ネットワークには触らない。
// finalized のフィクスチャは 2026-08-22 に本番の記録係が実際に綴じ、封じた記録。
// public_key と sig は一文字も違えてはならない（全角混入で一度事故が起きている）。

const REAL_ID = "c42e4c136d3eabb84543026568a67a59520ccec1107a595520266c95ed20a880";
const REAL_ENVELOPE = {
  finalized: true,
  text: "fukanzen-shogi-archive 1\nrule 0.7\nprotocol 5\napp 0.13.0\nsente -\ngote -\nresult resign draw\nsfen lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
  archived_at: "2026-08-22T04:30:32.536Z",
  witnesses: 2,
  seal: {
    alg: "Ed25519",
    kid: "2f05fdcbf64e2008",
    statement: "fukanzen-shogi-seal 1\nkind finalized\nid c42e4c136d3eabb84543026568a67a59520ccec1107a595520266c95ed20a880\nwitnesses 2\narchived_at 2026-08-22T04:30:32.536Z\nkid 2f05fdcbf64e2008",
    sig: "hBbLvP-F2BQZoCvjeU2XpWglrJ64QKfxe9pP1dH3-5-XvaFlFju-F13lhtdMjR_ZryhUQQ3tryRq9wXdr65rAA",
  },
};
const REAL_KEYS = [{
  kid: "2f05fdcbf64e2008", alg: "Ed25519",
  public_key: "ZgggVUj4KlZZYpYxDmykQuvU7ytr1UmvvQVvHf5t_dE",
}];

const realSubtle = globalThis.crypto.subtle;
const enc = new TextEncoder();

const hex = buf => Array.from(new Uint8Array(buf)).map(b => b.toString(16).padStart(2, '0')).join('');
const sha256HexBytes = async bytes => hex(await realSubtle.digest('SHA-256', bytes));
const sha256Hex      = text => sha256HexBytes(enc.encode(text));

function toBase64Url(bytes) {
  let s = '';
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

// 本番のフィクスチャを壊さずに一部だけ書き換えた複製を作る。
const withEnvelope = patch => ({ ...structuredClone(REAL_ENVELOPE), ...patch });
const withSeal = patch => {
  const e = structuredClone(REAL_ENVELOPE);
  Object.assign(e.seal, patch);
  return e;
};

// ── 本番の実物 ────────────────────────────────────────────────────────────────

describe("sealVerdict（本番が実際に綴じた記録）", () => {
  it("封蝋も中身も無傷なら verified", async () => {
    expect(await sealVerdict(REAL_ID, REAL_ENVELOPE, REAL_KEYS)).toBe('verified');
  });
});

// ── 改竄の検知 ────────────────────────────────────────────────────────────────

describe("sealVerdict（改竄の検知）", () => {
  it("本文を一文字変えると tampered（id が本文の SHA-256 と合わない）", async () => {
    const e = withEnvelope({ text: REAL_ENVELOPE.text.replace('rule 0.7', 'rule 0.8') });
    expect(await sealVerdict(REAL_ID, e, REAL_KEYS)).toBe('tampered');
  });

  it("平文 witnesses を書き換えると tampered（statement と食い違う）", async () => {
    const e = withEnvelope({ witnesses: 1 });
    expect(await sealVerdict(REAL_ID, e, REAL_KEYS)).toBe('tampered');
  });

  it("平文 archived_at を書き換えると tampered（statement と食い違う）", async () => {
    const e = withEnvelope({ archived_at: '2026-08-22T04:30:32.537Z' });
    expect(await sealVerdict(REAL_ID, e, REAL_KEYS)).toBe('tampered');
  });

  it("sig を差し替えると tampered（署名検証が落ちる）", async () => {
    const e = withSeal({ sig: 'i' + REAL_ENVELOPE.seal.sig.slice(1) });
    expect(await sealVerdict(REAL_ID, e, REAL_KEYS)).toBe('tampered');
  });

  it("statement の kid だけ書き換えると tampered（seal.kid と食い違う）", async () => {
    const e = withSeal({
      statement: REAL_ENVELOPE.seal.statement.replace('kid 2f05fdcbf64e2008', 'kid 0000000000000000'),
    });
    expect(await sealVerdict(REAL_ID, e, REAL_KEYS)).toBe('tampered');
  });

  it("別の id の記録として渡すと tampered（KV キーとの一致まで見る）", async () => {
    const otherId = 'f'.repeat(64);
    expect(await sealVerdict(otherId, REAL_ENVELOPE, REAL_KEYS)).toBe('tampered');
  });

  it("statement が壊れていると tampered（空白の無い行）", async () => {
    const e = withSeal({ statement: 'fukanzen-shogi-seal 1\nkind finalized\nこわれた行' });
    expect(await sealVerdict(REAL_ID, e, REAL_KEYS)).toBe('tampered');
  });
});

// ── 失敗ではない状態（「検証不可」と「改竄」を混同しない・不変の原則 8） ──────────

describe("sealVerdict（失敗ではない状態）", () => {
  it("封蝋が無ければ unsealed（導入前の記録・失敗ではない）", async () => {
    const e = structuredClone(REAL_ENVELOPE);
    delete e.seal;
    expect(await sealVerdict(REAL_ID, e, REAL_KEYS)).toBe('unsealed');
  });

  it("kid が公開鍵に無ければ unknown_key", async () => {
    expect(await sealVerdict(REAL_ID, REAL_ENVELOPE, [])).toBe('unknown_key');
    expect(await sealVerdict(REAL_ID, REAL_ENVELOPE, [
      { kid: '0000000000000000', alg: 'Ed25519', public_key: REAL_KEYS[0].public_key },
    ])).toBe('unknown_key');
  });

  it("公開鍵の材が壊れていれば unknown_key（unsupported ではない＝公開口の問題）", async () => {
    // 実際に踏んだ事故の再現——公開鍵へ全角文字が一文字紛れた（先頭が全角 Ｚ）。
    const keys = [{ ...REAL_KEYS[0], public_key: 'Ｚ' + REAL_KEYS[0].public_key.slice(1) }];
    expect(await sealVerdict(REAL_ID, REAL_ENVELOPE, keys)).toBe('unknown_key');
  });

  it("未知の alg は unsupported", async () => {
    const e = withSeal({ alg: 'RSASSA-PKCS1-v1_5' });
    expect(await sealVerdict(REAL_ID, e, REAL_KEYS)).toBe('unsupported');
  });

  it("未来の封蝋書式版は unsupported（tampered ではない＝読めないだけ）", async () => {
    const e = withSeal({
      statement: REAL_ENVELOPE.seal.statement.replace('fukanzen-shogi-seal 1', 'fukanzen-shogi-seal 2'),
    });
    expect(await sealVerdict(REAL_ID, e, REAL_KEYS)).toBe('unsupported');
  });

  it("Ed25519 を扱えない環境は unsupported（importKey が投げる）", async () => {
    const noEd25519 = {
      digest: (...args) => realSubtle.digest(...args),
      importKey: async () => { throw new Error('Ed25519 は未対応'); },
      verify: async () => { throw new Error('ここへ来てはならない'); },
    };
    expect(await sealVerdict(REAL_ID, REAL_ENVELOPE, REAL_KEYS, noEd25519)).toBe('unsupported');
  });

  it("subtle が無ければ unsupported（null を渡すこと・undefined は既定値が効く）", async () => {
    expect(await sealVerdict(REAL_ID, REAL_ENVELOPE, REAL_KEYS, null)).toBe('unsupported');
  });
});

// ── disputed（本番に実例が無いので、その場で鍵を作って封じる） ───────────────────

describe("sealVerdict（disputed・証言の不一致）", () => {
  let ID, ENVELOPE, KEYS, TEXT_A, TEXT_B;

  beforeAll(async () => {
    const kp = await realSubtle.generateKey({ name: 'Ed25519' }, true, ['sign', 'verify']);
    const raw = new Uint8Array(await realSubtle.exportKey('raw', kp.publicKey));
    const kid = (await sha256HexBytes(raw)).slice(0, 16);

    TEXT_A = 'fukanzen-shogi-archive 1\nrule 0.7\nprotocol 5\napp 0.13.0\nsente -\ngote -\nresult resign gote_wins\nsfen 9/9/9/9/9/9/9/9/9 b - 1';
    TEXT_B = 'fukanzen-shogi-archive 1\nrule 0.7\nprotocol 5\napp 0.13.0\nsente -\ngote -\nresult resign sente_wins\nsfen 9/9/9/9/9/9/9/9/9 b - 1';
    ID = '7b6f5e4d-3c2b-1a09-8f7e-6d5c4b3a2190';   // 暫定 UUID（content-address ではない）
    const archivedAt = '2026-08-22T05:12:00.000Z';

    const statement = [
      'fukanzen-shogi-seal 1',
      'kind disputed',
      `id ${ID}`,
      `text_a ${await sha256Hex(TEXT_A)}`,
      `text_b ${await sha256Hex(TEXT_B)}`,
      `archived_at ${archivedAt}`,
      `kid ${kid}`,
    ].join('\n');
    const sig = toBase64Url(new Uint8Array(
      await realSubtle.sign({ name: 'Ed25519' }, kp.privateKey, enc.encode(statement))));

    ENVELOPE = {
      disputed: true,
      texts: [TEXT_A, TEXT_B],
      archived_at: archivedAt,
      seal: { alg: 'Ed25519', kid, statement, sig },
    };
    KEYS = [{ kid, alg: 'Ed25519', public_key: toBase64Url(raw) }];
  });

  it("両証言が封蝋どおりなら verified", async () => {
    expect(await sealVerdict(ID, ENVELOPE, KEYS)).toBe('verified');
  });

  it("証言 A を書き換えると tampered", async () => {
    const e = { ...ENVELOPE, texts: [TEXT_A + '\n', TEXT_B] };
    expect(await sealVerdict(ID, e, KEYS)).toBe('tampered');
  });

  it("両証言を入れ替えると tampered（順序も封じられている）", async () => {
    const e = { ...ENVELOPE, texts: [TEXT_B, TEXT_A] };
    expect(await sealVerdict(ID, e, KEYS)).toBe('tampered');
  });
});

// ── parseStatement ────────────────────────────────────────────────────────────

describe("parseStatement（行ベース statement の分解）", () => {
  it("正しい statement を field へ分解する（値は文字列・version だけ数値）", () => {
    expect(parseStatement(REAL_ENVELOPE.seal.statement)).toEqual({
      version: 1,
      kind: 'finalized',
      id: REAL_ID,
      witnesses: '2',
      archived_at: '2026-08-22T04:30:32.536Z',
      kid: '2f05fdcbf64e2008',
    });
  });

  it("magic が違えば null", () => {
    expect(parseStatement('fukanzen-shogi-sealed 1\nkind finalized')).toBeNull();
  });

  it("空白の無い行があれば null", () => {
    expect(parseStatement('fukanzen-shogi-seal 1\nkind finalized\nwitnesses')).toBeNull();
  });

  it("文字列でなければ null", () => {
    expect(parseStatement(null)).toBeNull();
    expect(parseStatement(undefined)).toBeNull();
    expect(parseStatement({ kind: 'finalized' })).toBeNull();
  });
});
