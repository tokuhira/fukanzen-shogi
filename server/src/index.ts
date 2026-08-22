import { GameRoom } from "./room";
import { loadRecorderKey, toBase64Url } from "./logic";

export { GameRoom };

interface Env {
  ROOM: DurableObjectNamespace;
  SPECTATE_TOKENS: KVNamespace;
  ARCHIVES: KVNamespace;
  RECORDER_SIGNING_KEY?: string;
}

// 書庫も公開鍵も誰でも読める公開データ（版図「公開情報は対等、綴じ込みだけが特権」）。
// * はその設計を正直に表明するもので、オリジンを絞る意味は無い(実装指示書 §2.3)。
const PUBLIC_CORS = { "Access-Control-Allow-Origin": "*" };

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);

    // 書庫からの直接取り出し: GET /archive/:id（部屋 DO を介さない。書庫は
    // 部屋のライフサイクルから独立。記録係一段目 §7）。
    const archiveMatch = url.pathname.match(/^\/archive\/([^/]+)$/);
    if (archiveMatch && request.method === "GET") {
      const id = decodeURIComponent(archiveMatch[1]);
      const raw = await env.ARCHIVES.get(id);
      if (!raw) {
        // 404 にも CORS が要る——無いと Seal-4 が「記録が無い」と
        // 「ネットワークが死んだ」を区別できない（実装指示書 §2.1）。
        return new Response("Not found", { status: 404, headers: { ...PUBLIC_CORS } });
      }
      return new Response(raw, {
        headers: { "Content-Type": "application/json", ...PUBLIC_CORS },
      });
    }

    // 記録係の公開鍵（記録係三段目 Seal-3）。封蝋を第三者が検証するための口。
    // DO を介さない。配列で返すのは継ぎ目の予約——鍵ローテーションと、遠い地平の
    // 複数記録係が、この形のまま載る（概観 §3・プリンシパル設計 §5）。中身は今は書かない。
    if (url.pathname === "/recorder/keys" && request.method === "GET") {
      let keys: unknown[] = [];
      try {
        const key = await loadRecorderKey(env);
        if (key) {
          keys = [{ kid: key.kid, alg: "Ed25519", public_key: toBase64Url(key.rawPublicKey) }];
        }
      } catch (err) {
        // 鍵が壊れている＝設定ミス。ログには残すが、応答は「今は封をしていない」で
        // 揃える——このとき Seal-2 も封蝋なしで綴じているので、系として整合する
        // （封蝋が無い記録に、封蝋を検証する鍵を返しても意味がない）。
        console.log(`[recorder/keys] key load failed: ${String(err)}`);
      }
      return new Response(JSON.stringify({ keys }, null, 2), {
        headers: {
          "Content-Type": "application/json",
          "Cache-Control": "public, max-age=300",
          ...PUBLIC_CORS,
        },
      });
    }

    // 観戦: /watch/:token → KV で roomKey に解決し、該当 DO へ spectator として委譲
    // （room key を知らせず、読み取り専用の別トークンで入れる。淀川第三歩 §4）。
    const watchMatch = url.pathname.match(/^\/watch\/([^/]+)$/);
    if (watchMatch) {
      const token = decodeURIComponent(watchMatch[1]);
      const roomKey = await env.SPECTATE_TOKENS.get(token);
      if (!roomKey) {
        return new Response("Not found", { status: 404 });
      }
      const id = env.ROOM.idFromName(roomKey);
      const stub = env.ROOM.get(id);
      const spectateUrl = new URL(request.url);
      spectateUrl.pathname = `/room/${encodeURIComponent(roomKey)}/spectate`;
      const forwarded = new Request(spectateUrl.toString(), request);
      return stub.fetch(forwarded);
    }

    const match = url.pathname.match(/^\/room\/([^/]+?)(\/status|\/archive|\/spectate)?$/);
    if (!match) {
      return new Response("Not found", { status: 404 });
    }

    const roomKey = decodeURIComponent(match[1]);
    const id = env.ROOM.idFromName(roomKey);
    const stub = env.ROOM.get(id);

    return stub.fetch(request);
  },
} satisfies ExportedHandler<Env>;
