// node（vitest）で wasm-bindgen 生成モジュールを読むテスト専用ヘルパ。
// 本番の board.js は従来通りブラウザの fetch 経路で読む——ここはテストだけの入口。
// init に { module_or_path: bytes } を渡すと fetch 分岐を通らず WebAssembly.instantiate
// に直行する（bytes を裸で渡すと deprecated 警告が出るのでオブジェクト形で渡す）。

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

async function loadWasm(dir, base) {
  const bytes = readFileSync(
    fileURLToPath(new URL(`../${dir}/${base}_bg.wasm`, import.meta.url))
  );
  const mod = await import(`../${dir}/${base}.js`);
  await mod.default({ module_or_path: bytes });
  return mod;
}

// 遅延ロード（呼ばれたテストだけが実 Wasm を読む）。
export const loadEngine   = () => loadWasm("wasm",          "engine_wasm");
export const loadNotation = () => loadWasm("notation-wasm", "notation_wasm");
export const loadProtocol = () => loadWasm("protocol-wasm", "protocol_wasm");
