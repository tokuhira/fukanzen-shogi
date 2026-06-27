export class GameRoom implements DurableObject {
  private state: DurableObjectState;

  constructor(state: DurableObjectState) {
    this.state = state;
  }

  async fetch(request: Request): Promise<Response> {
    if (request.headers.get("Upgrade") !== "websocket") {
      return new Response("WebSocket required", { status: 426 });
    }

    const existing = this.state.getWebSockets();
    const gameStarted =
      (await this.state.storage.get<boolean>("gameStarted")) ?? false;

    if (existing.length >= 2) {
      return new Response(JSON.stringify({ type: "room_full" }), {
        status: 403,
        headers: { "Content-Type": "application/json" },
      });
    }

    const { 0: client, 1: server } = new WebSocketPair();
    this.state.acceptWebSocket(server);

    if (!gameStarted) {
      // 新規ゲームフロー
      if (existing.length === 1) {
        // 2人目が入室 → 先後確定
        existing[0].send(JSON.stringify({ type: "peer_joined", your_side: "sente" }));
        server.send(JSON.stringify({ type: "room_ready", your_side: "gote" }));
        await this.state.storage.put("gameStarted", true);
      }
      // 1人目は入室待ち → 何も送らない
    } else {
      // 対局開始済み → 再接続フロー
      if (existing.length === 1) {
        // 残留プレイヤーへ通知
        existing[0].send(JSON.stringify({ type: "peer_reconnected" }));
        // 再接続プレイヤーへ通知
        server.send(JSON.stringify({ type: "you_reconnected" }));
      }
      // existing.length === 0: 両者切断中 → 次の再接続者が来たら片方だけいる状態になる
    }

    return new Response(null, { status: 101, webSocket: client });
  }

  webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): void {
    for (const other of this.state.getWebSockets()) {
      if (other !== ws) {
        other.send(message);
      }
    }
  }

  webSocketClose(ws: WebSocket): void {
    for (const other of this.state.getWebSockets()) {
      if (other !== ws) {
        try {
          other.send(JSON.stringify({ type: "peer_disconnected" }));
        } catch {
          // other socket already closed
        }
      }
    }
  }

  webSocketError(ws: WebSocket): void {
    this.webSocketClose(ws);
  }
}
