interface Env {
  SPECTATE_TOKENS: KVNamespace;
  ARCHIVES: KVNamespace;
}

interface SpectateTurn {
  s: string;
  g: string;
}

interface SpectateResult {
  kind: string;
  outcome: string;
}

interface SpectateRecord {
  version: unknown;
  initial_sfen: string | null;
  turns: SpectateTurn[];
  result: SpectateResult | null;
  archived: boolean;
}

// ルール v0.6 の最長手数（engine::terminate::MAX_TURNS）と同じ上限。悪意ある/壊れた
// player ソケットが spectate_turn を無制限に送りつけて turns 配列を肥大化させるのを防ぐ。
const MAX_TURNS = 500;

// web/board.js の MAX_ARCHIVE_BYTES と同じ上限。spectate_result の text を書庫へ確定綴じ
// する前のサイズ上限（KV の値サイズ上限や無駄な帯域消費を避ける）。
const MAX_ARCHIVE_TEXT_BYTES = 512 * 1024;

async function sha256Hex(text: string): Promise<string> {
  const data = new TextEncoder().encode(text);
  const digest = await crypto.subtle.digest("SHA-256", data);
  return Array.from(new Uint8Array(digest))
    .map(b => b.toString(16).padStart(2, "0"))
    .join("");
}

export class GameRoom implements DurableObject {
  private state: DurableObjectState;
  private env: Env;
  private readonly _key: string;

  constructor(state: DurableObjectState, env: Env) {
    this.state = state;
    this.env = env;
    this._key = state.id.name ?? '?';
  }

  private log(...args: unknown[]): void {
    console.log(`[room:${this._key}]`, ...args);
  }

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);

    // 診断エンドポイント: GET /room/:key/status
    if (request.method === "GET" && url.pathname.endsWith("/status")) {
      const gameStarted = (await this.state.storage.get<boolean>("gameStarted")) ?? false;
      const players = this.state.getWebSockets("player").length;
      const spectators = this.state.getWebSockets("spectator").length;
      return new Response(JSON.stringify({ gameStarted, players, spectators }, null, 2), {
        headers: { "Content-Type": "application/json" },
      });
    }

    // サーバ側アーカイブの取り出し: GET /room/:key/archive（淀川第三歩 §6）。
    // 部屋のいまの一局（ライブ・診断用）。恒久の記録は /archive/:id（記録係一段目 §7）。
    if (request.method === "GET" && url.pathname.endsWith("/archive")) {
      const record = await this._loadRecord();
      return new Response(JSON.stringify(record, null, 2), {
        headers: { "Content-Type": "application/json" },
      });
    }

    if (request.headers.get("Upgrade") !== "websocket") {
      return new Response("WebSocket required", { status: 426 });
    }

    // 観戦者接続（/watch/:token 経由。index.ts が /room/:key/spectate へ書き換えて委譲）。
    // 枠制限なし・読み取り専用（淀川第三歩 §3.1・§4）。
    if (url.pathname.endsWith("/spectate")) {
      const { 0: client, 1: server } = new WebSocketPair();
      this.state.acceptWebSocket(server, ["spectator"]);
      const record = await this._loadRecord();
      server.send(JSON.stringify({ type: "spectate_init", ...record }));
      this.log("spectator connected");
      return new Response(null, { status: 101, webSocket: client });
    }

    // 2人枠は player のみで計数（観戦者は枠外。淀川第三歩 §3.1）。
    const existing = this.state.getWebSockets("player");
    const gameStarted =
      (await this.state.storage.get<boolean>("gameStarted")) ?? false;

    this.log(`connect existing=${existing.length} gameStarted=${gameStarted}`);

    if (existing.length >= 2) {
      this.log("rejected: room_full");
      return new Response(JSON.stringify({ type: "room_full" }), {
        status: 403,
        headers: { "Content-Type": "application/json" },
      });
    }

    const { 0: client, 1: server } = new WebSocketPair();
    this.state.acceptWebSocket(server, ["player"]);

    if (!gameStarted) {
      if (existing.length === 1) {
        existing[0].send(JSON.stringify({ type: "peer_joined", your_side: "sente" }));
        server.send(JSON.stringify({ type: "room_ready", your_side: "gote" }));
        await this.state.storage.put("gameStarted", true);
        this.log("game started: sente + gote assigned");
      } else {
        this.log("waiting for 2nd player");
      }
    } else {
      if (existing.length === 1) {
        existing[0].send(JSON.stringify({ type: "peer_reconnected" }));
        server.send(JSON.stringify({ type: "you_reconnected" }));
        this.log("reconnect flow: you_reconnected sent");
        this._broadcastSpectateStatus("resumed");
      } else {
        await this.state.storage.delete("gameStarted");
        this.log("stale gameStarted cleared (existing=0)");
      }
    }

    return new Response(null, { status: 101, webSocket: client });
  }

  // 送信者の役割とメッセージ型で振り分ける（淀川第三歩 §1・§3.2 の秘匿境界の急所）。
  // - player 送信の spectate_meta/turn/result → 観戦者へ fan-out ＋ 記録へ反映。
  //   相手プレイヤーへの転送は不要（相手は自分で公開組手を知っている）。
  // - player 送信のそれ以外（commit/reveal/ack/hello/reconnect 等）→ 相手プレイヤー
  //   のみへ転送。観戦者へは絶対に送らない。
  // - spectator 送信 → 一切転送しない（読み取り専用。入力は破棄）。
  async webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): Promise<void> {
    if (typeof message !== "string") return;

    let msg: { type?: string; [k: string]: unknown };
    try {
      msg = JSON.parse(message);
    } catch {
      return;
    }
    this.log(`recv type=${msg.type ?? "unknown"}`);

    // 観戦者は読み取り専用。型を見るより先に、ここで無条件に破棄する（淀川第三歩
    // §1-B・§10.1）。以前は request_reset の分岐がこのチェックより先にあったため、
    // 観戦者が request_reset を送ると getWebSockets("player") 全員が
    // 「other !== ws」を満たしてしまい（観戦ソケットは player リストに元々
    // 含まれないため）、対局を強制リセットできてしまっていた。
    if (this.state.getWebSockets("spectator").includes(ws)) return;

    if (msg.type === "request_reset") {
      this.log("request_reset: clearing gameStarted and closing other players");
      void this.state.storage.delete("gameStarted");
      for (const other of this.state.getWebSockets("player")) {
        if (other !== ws) {
          try { other.close(1000, "room reset"); } catch {}
        }
      }
      ws.close(1000, "room reset");
      return;
    }

    if (msg.type === "spectate_meta" || msg.type === "spectate_turn" || msg.type === "spectate_result") {
      await this._applyToRecord(msg);
      if (msg.type === "spectate_meta") {
        await this._issueSpectateToken();
      }
      for (const spec of this.state.getWebSockets("spectator")) {
        try { spec.send(message); } catch {}
      }
      return;
    }

    // 対局チャネル（commit/reveal/ack/hello/reconnect など）は相手プレイヤーのみへ。
    for (const other of this.state.getWebSockets("player")) {
      if (other !== ws) {
        other.send(message);
      }
    }
  }

  async webSocketClose(ws: WebSocket): Promise<void> {
    const wasSpectator = this.state.getWebSockets("spectator").includes(ws);
    if (wasSpectator) {
      // 観戦者自身の切断は記録に影響しない（淀川第三歩 §3.4）。
      return;
    }

    const otherPlayers = this.state.getWebSockets("player").filter(s => s !== ws);
    this.log(`close remaining players=${otherPlayers.length}`);

    for (const other of otherPlayers) {
      try {
        other.send(JSON.stringify({ type: "peer_disconnected" }));
      } catch {}
    }
    this._broadcastSpectateStatus("player_disconnected");

    if (otherPlayers.length === 0) {
      // 全員離脱（記録係一段目 §4-3）。gameStarted を消す前に、綴じていない
      // 記録があれば断片として書庫へ綴じる（「綴じてから拭く」不変条件）。
      await this._archiveCurrentIfNeeded();
      void this.state.storage.delete("gameStarted");
      this.log("all players disconnected: gameStarted cleared");
    }
  }

  webSocketError(ws: WebSocket): void {
    this.log("webSocketError → webSocketClose");
    void this.webSocketClose(ws);
  }

  // ── 公開組手の記録（淀川第三歩 §3.3・§6） ────────────────────────────────────

  private async _loadRecord(): Promise<SpectateRecord> {
    const version = (await this.state.storage.get<unknown>("version")) ?? null;
    const initial_sfen = (await this.state.storage.get<string>("initial_sfen")) ?? null;
    const turns = (await this.state.storage.get<SpectateTurn[]>("turns")) ?? [];
    const result = (await this.state.storage.get<SpectateResult>("result")) ?? null;
    const archived = (await this.state.storage.get<boolean>("archived")) ?? false;
    return { version, initial_sfen, turns, result, archived };
  }

  private async _applyToRecord(msg: { type?: string; [k: string]: unknown }): Promise<void> {
    if (msg.type === "spectate_meta") {
      // 新しい対局の開始（再戦を含む）。記録係一段目 §4-2: 拭く（初期化する）前に、
      // 前局が未綴じのまま残っていれば断片として書庫へ綴じる。
      await this._archiveCurrentIfNeeded();

      await this.state.storage.put("version", msg.version ?? null);
      await this.state.storage.put("initial_sfen", (msg.initial_sfen as string) ?? null);
      await this.state.storage.put("turns", []);
      await this.state.storage.put("result", null);
      await this.state.storage.put("archived", false);
    } else if (msg.type === "spectate_turn") {
      const turns = (await this.state.storage.get<SpectateTurn[]>("turns")) ?? [];
      if (turns.length < MAX_TURNS) {
        turns.push({ s: String(msg.s ?? ""), g: String(msg.g ?? "") });
        await this.state.storage.put("turns", turns);
      }
    } else if (msg.type === "spectate_result") {
      const result = { kind: String(msg.kind ?? ""), outcome: String(msg.outcome ?? "") };
      await this.state.storage.put("result", result);

      const recordForFallback: SpectateRecord = {
        version: (await this.state.storage.get<unknown>("version")) ?? null,
        initial_sfen: (await this.state.storage.get<string>("initial_sfen")) ?? null,
        turns: (await this.state.storage.get<SpectateTurn[]>("turns")) ?? [],
        result,
        archived: false,
      };

      // 記録係一段目 §4-1・§6: 正準本文（text）があり上限内であれば内容ハッシュで
      // 確定綴じ。無ければ（旧クライアント・上限超過等）現レコードを断片として
      // 綴じる——いずれの経路でもデータは失われない。
      //
      // archived フラグは、書庫への書き込み（KV I/O）が実際に成功した後にのみ
      // 立てる。先に立てると、書き込みが失敗した場合に「綴じ済み」を名乗り
      // ながら実体が存在しないサイレントな記録喪失になる（webSocketClose 側の
      // _archiveCurrentIfNeeded はもう再試行しない）。webSocketClose との
      // 競合で断片が二重に綴じられる可能性は残るが、二重書き込みは無害であり、
      // サイレント喪失よりずっと軽い代償。
      const text = typeof msg.text === "string" && msg.text.length <= MAX_ARCHIVE_TEXT_BYTES
        ? msg.text
        : null;
      try {
        const id = text
          ? await this._archiveFinalized(text)
          : await this._archiveFragment(recordForFallback);
        await this.state.storage.put("archived", true);
        this._broadcastArchived(id);
      } catch (err) {
        this.log(`archive write failed: ${String(err)}`);
      }
    }
  }

  // ── 書庫（記録係一段目 §2・§3・§4） ──────────────────────────────────────────

  // 未綴じ（archived=false）かつ着手のある記録を、断片として書庫へ綴じる。
  // 再戦開始（§4-2）・全員離脱（§4-3）の両方から呼ぶ共通経路。
  private async _archiveCurrentIfNeeded(): Promise<void> {
    const archived = (await this.state.storage.get<boolean>("archived")) ?? false;
    if (archived) return;
    const turns = (await this.state.storage.get<SpectateTurn[]>("turns")) ?? [];
    if (turns.length === 0) return;

    const record = await this._loadRecord();
    await this._archiveFragment(record);
    await this.state.storage.put("archived", true);
  }

  // 確定局: 正準アーカイブ本文の内容ハッシュ（SHA-256）を id として書庫へ綴じる。
  // DO は本文を解さない（不透明ブロブとして content-address するだけ）。
  private async _archiveFinalized(text: string): Promise<string> {
    const id = await sha256Hex(text);
    const envelope = {
      finalized: true,
      text,
      archived_at: new Date().toISOString(),
    };
    await this.env.ARCHIVES.put(id, JSON.stringify(envelope));
    this.log(`archived finalized id=${id}`);
    return id;
  }

  // 放棄断片: 確定していない対局を暫定 ID（ランダム UUID）で書庫へ綴じる。
  private async _archiveFragment(record: SpectateRecord): Promise<string> {
    const id = crypto.randomUUID();
    const envelope = {
      finalized: false,
      record: {
        version: record.version,
        initial_sfen: record.initial_sfen,
        turns: record.turns,
        result: record.result,
      },
      archived_at: new Date().toISOString(),
    };
    await this.env.ARCHIVES.put(id, JSON.stringify(envelope));
    this.log(`archived fragment id=${id}`);
    return id;
  }

  private _broadcastArchived(id: string): void {
    const payload = JSON.stringify({ type: "archived", id });
    for (const player of this.state.getWebSockets("player")) {
      try { player.send(payload); } catch {}
    }
    for (const spec of this.state.getWebSockets("spectator")) {
      try { spec.send(payload); } catch {}
    }
  }

  // ── 観戦トークン（淀川第三歩 §4） ────────────────────────────────────────────

  private async _issueSpectateToken(): Promise<void> {
    const previous = await this.state.storage.get<string>("spectateToken");
    if (previous) {
      // 古いリンクを無効化（新しい対局が始まったため）。
      try { await this.env.SPECTATE_TOKENS.delete(previous); } catch {}
    }

    const token = crypto.randomUUID();
    await this.env.SPECTATE_TOKENS.put(token, this._key, { expirationTtl: 60 * 60 * 24 * 30 });
    await this.state.storage.put("spectateToken", token);

    const payload = JSON.stringify({ type: "spectate_token", token });
    for (const player of this.state.getWebSockets("player")) {
      try { player.send(payload); } catch {}
    }
    this.log("spectate token issued");
  }

  private _broadcastSpectateStatus(state: "player_disconnected" | "resumed"): void {
    const payload = JSON.stringify({ type: "spectate_status", state });
    for (const spec of this.state.getWebSockets("spectator")) {
      try { spec.send(payload); } catch {}
    }
  }
}
