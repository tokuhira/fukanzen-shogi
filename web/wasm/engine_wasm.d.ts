/* tslint:disable */
/* eslint-disable */

/**
 * 対局データを版タプル付きアーカイブ書式 v1 のテキストへ変換する。
 *
 * request_json:
 * `{"initial_sfen":"...","plies":[{"s":"7g7f","g":"3c3d"},...],
 *   "rule":"0.5","protocol":2,"app":"0.8.0","sente":null,"gote":null,
 *   "result":{"kind":"mate","outcome":"gote_wins"}}`
 *
 * 成功: アーカイブ本文の文字列
 * 失敗: `"ERROR: <理由>"`
 */
export function build_archive(request_json: string): string;

/**
 * 棋譜（初期局面＋着手列）から盤上の終局を評価する（投了を除く。ルール v0.6 §5.8）。
 * `build_archive` と同じ流儀で initial_sfen＋plies から Kifu を構成し、
 * `engine::terminate::evaluate` を呼んで、結果を archive の語彙
 * （`ResultKind`/`Outcome`）に対応づけて返す。
 *
 * request_json: `{"initial_sfen":"...","plies":[{"s":"7g7f","g":"3c3d"}, ...]}`
 *
 * 成功: `{"status":"ongoing"}` または
 *       `{"status":"terminal","kind":"mate","outcome":"gote_wins"}`
 * 失敗: `{"status":"error","error":"<理由>"}`
 */
export function evaluate_terminal(request_json: string): string;

/**
 * 指定局面のゲーム状態を返す（着手選択前の確定詰みチェック）。
 *
 * 返値: "ongoing" | "sente_loses" | "gote_loses" | "draw" | "error"
 */
export function game_status(sfen: string): string;

/**
 * 指定局面・陣営の合法手を USI 文字列の JSON 配列として返す。
 *
 * - sfen: 局面の SFEN 文字列
 * - side: "sente" | "gote"
 *
 * 返値: `["7g7f","P*5e",...]`（空なら `[]`）
 */
export function legal_actions(sfen: string, side: string): string;

/**
 * ルール v0.6 の最長手数（組手）。`engine::terminate::MAX_TURNS` が単一の値であり、
 * アーカイブ読込の安全網（`parse_archive`）もここから参照する（ハードコードの
 * 重複を持たない）。web 側もこの getter から値を取得し、JS 側に定数を複製しない。
 */
export function max_turns(): number;

/**
 * アーカイブ書式 v1（または旧 sfen 始まり）のテキストを解釈して対局データを返す。
 * `build_archive` の対。
 *
 * 成功: `{"ok":true,"initial_sfen":"...","plies":[{"s":"7g7f","g":"3c3d"},...],
 *        "meta":{"rule":"0.5","protocol":2,"app":"0.8.0","sente":null,"gote":null,
 *                "result":{"kind":"mate","outcome":"gote_wins"}}}`
 * 失敗: `{"ok":false,"error":"<理由>"}`（着手数超過時は `"too_many_plies"`）
 */
export function parse_archive(text: string): string;

/**
 * SFEN を解釈し、描画に必要な構造化盤面を JSON で返す。
 * `engine::serialize::sfen_to_position` を再利用する（SFEN 解釈の単一の正本。
 * web/board.js の自前 `parseSfen` の重複を解消する——board.js 分割 第〇段）。
 *
 * 返値（成功）:
 * `{"board":[{"file":2,"rank":8,"kind":"R","side":"s"}, ...],
 *   "hand_s":{"P":2,"G":1},"hand_g":{"P":1}}`
 * （`board` は駒のあるマスのみ。file は 9〜1・rank は 1〜9、SFEN の座標に一致）
 *
 * 返値（失敗）: `{"error":"bad_sfen"}`
 */
export function position_view(sfen: string): string;

/**
 * 両着手を解決して次局面と発生事象を返す。
 *
 * - sfen: 現局面の SFEN 文字列
 * - sente_usi: 先手の USI 着手（例: "7g7f", "P*8f", "8h3c+"）
 * - gote_usi:  後手の USI 着手
 *
 * 成功: `{"ok":true,"sfen":"<次局面>","event":"normal|clash|sente_died|gote_died|both_died"}`
 * 失敗: `{"ok":false,"error":"<理由>"}`
 */
export function resolve_ply(sfen: string, sente_usi: string, gote_usi: string): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly build_archive: (a: number, b: number) => [number, number];
    readonly evaluate_terminal: (a: number, b: number) => [number, number];
    readonly game_status: (a: number, b: number) => [number, number];
    readonly legal_actions: (a: number, b: number, c: number, d: number) => [number, number];
    readonly max_turns: () => number;
    readonly parse_archive: (a: number, b: number) => [number, number];
    readonly position_view: (a: number, b: number) => [number, number];
    readonly resolve_ply: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number];
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
