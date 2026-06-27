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

    if (existing.length >= 2) {
      return new Response(JSON.stringify({ type: "room_full" }), {
        status: 403,
        headers: { "Content-Type": "application/json" },
      });
    }

    const { 0: client, 1: server } = new WebSocketPair();
    this.state.acceptWebSocket(server);

    if (existing.length === 1) {
      // 1人目(先手)に peer_joined、2人目(後手)に room_ready を通知
      existing[0].send(JSON.stringify({ type: "peer_joined", your_side: "sente" }));
      server.send(JSON.stringify({ type: "room_ready", your_side: "gote" }));
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
