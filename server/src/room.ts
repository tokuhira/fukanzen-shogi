interface Env {
  SPECTATE_TOKENS: KVNamespace;
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

    const isSpectator = this.state.getWebSockets("spectator").includes(ws);
    if (isSpectator) {
      // 観戦者は読み取り専用。入力は破棄する（淀川第三歩 §1-B）。
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

  webSocketClose(ws: WebSocket): void {
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
      void this.state.storage.delete("gameStarted");
      this.log("all players disconnected: gameStarted cleared");
    }
  }

  webSocketError(ws: WebSocket): void {
    this.log("webSocketError → webSocketClose");
    this.webSocketClose(ws);
  }

  // ── 公開組手の記録（淀川第三歩 §3.3・§6） ────────────────────────────────────

  private async _loadRecord(): Promise<SpectateRecord> {
    const version = (await this.state.storage.get<unknown>("version")) ?? null;
    const initial_sfen = (await this.state.storage.get<string>("initial_sfen")) ?? null;
    const turns = (await this.state.storage.get<SpectateTurn[]>("turns")) ?? [];
    const result = (await this.state.storage.get<SpectateResult>("result")) ?? null;
    return { version, initial_sfen, turns, result };
  }

  private async _applyToRecord(msg: { type?: string; [k: string]: unknown }): Promise<void> {
    if (msg.type === "spectate_meta") {
      // 新しい対局の開始（再戦を含む）。記録を初期化する。
      await this.state.storage.put("version", msg.version ?? null);
      await this.state.storage.put("initial_sfen", (msg.initial_sfen as string) ?? null);
      await this.state.storage.put("turns", []);
      await this.state.storage.put("result", null);
    } else if (msg.type === "spectate_turn") {
      const turns = (await this.state.storage.get<SpectateTurn[]>("turns")) ?? [];
      turns.push({ s: String(msg.s ?? ""), g: String(msg.g ?? "") });
      await this.state.storage.put("turns", turns);
    } else if (msg.type === "spectate_result") {
      await this.state.storage.put("result", {
        kind: String(msg.kind ?? ""),
        outcome: String(msg.outcome ?? ""),
      });
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
