//! 対局チャネル（DO の routeDecision で言う "other_player_only"）のワイヤ語彙。
//! JSON の唯一の正本。hello / commit / reveal / ack / reconnect / reconnect_ack / abort。
//! DO のシステム・部屋メッセージ（peer_joined 等）は含めない——それは層D（殻）の関心事。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WireMessage {
    /// 接続直後のハンドシェイク。版＋認証ハッシュ＋陣営を一通に集約。
    Hello {
        rule_major: u32,
        rule_minor: u32,
        proto_ver: u32,
        auth_hash: String, // hex(SHA-256(secret))
        side: String,       // "sente" | "gote"
    },
    /// commit フェーズ。
    Commit { commitment: String }, // hex, 32byte
    /// reveal フェーズ。着手欄は `action`（USI 文字列）。
    Reveal {
        action: String,     // USI
        nonce: String,      // hex, 32byte
        board_hash: String, // hex, 32byte
    },
    /// ack フェーズ。
    Ack,
    /// 再接続ハンドシェイク。生 secret は晒さず auth_hash を送る。
    Reconnect {
        auth_hash: String,  // hex
        board_hash: String, // hex（現局面）
    },
    /// 再接続の承認応答（再開点の board_hash）。
    ReconnectAck { board_hash: String }, // hex
    /// プロトコル違反・版不一致・認証失敗によるアボート。
    Abort { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireError {
    InvalidJson,
    UnknownType, // 対局チャネル外の type（DO システムメッセージ等が誤って渡った）
}

impl WireMessage {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("WireMessage serialize は無謬")
    }

    /// 対局チャネルの WireMessage を厳密に解釈する。
    /// 未知 type（peer_joined 等 DO システムメッセージを含む）は `UnknownType`。
    pub fn from_json(s: &str) -> Result<WireMessage, WireError> {
        match serde_json::from_str::<WireMessage>(s) {
            Ok(m) => Ok(m),
            Err(_) => {
                if serde_json::from_str::<serde_json::Value>(s).is_ok() {
                    Err(WireError::UnknownType)
                } else {
                    Err(WireError::InvalidJson)
                }
            }
        }
    }
}

// ── hex ⇄ バイト列ヘルパー ──────────────────────────────────────────────────
// net.rs（to_hex/from_hex/commitment_from_hex/nonce_from_hex/board_hash_from_hex）と
// protocol-wasm（to_hex/from_hex32）の重複の正本。寄せは第二・三段で行う。

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

pub fn from_hex32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(msg: WireMessage) {
        let json = msg.to_json();
        let back = WireMessage::from_json(&json).expect("往復は成功するはず");
        assert_eq!(back, msg);
    }

    #[test]
    fn roundtrip_all_variants() {
        roundtrip(WireMessage::Hello {
            rule_major: 0,
            rule_minor: 6,
            proto_ver: 5,
            auth_hash: "a".repeat(64),
            side: "sente".to_string(),
        });
        roundtrip(WireMessage::Commit {
            commitment: "b".repeat(64),
        });
        roundtrip(WireMessage::Reveal {
            action: "7g7f".to_string(),
            nonce: "c".repeat(64),
            board_hash: "d".repeat(64),
        });
        roundtrip(WireMessage::Ack);
        roundtrip(WireMessage::Reconnect {
            auth_hash: "e".repeat(64),
            board_hash: "f".repeat(64),
        });
        roundtrip(WireMessage::ReconnectAck {
            board_hash: "0".repeat(64),
        });
        roundtrip(WireMessage::Abort {
            reason: "version_mismatch".to_string(),
        });
    }

    #[test]
    fn byte_layout_matches_protocol_wasm() {
        let hello = WireMessage::Hello {
            rule_major: 0,
            rule_minor: 6,
            proto_ver: 5,
            auth_hash: "ab".repeat(32),
            side: "sente".to_string(),
        };
        assert_eq!(
            hello.to_json(),
            format!(
                r#"{{"type":"hello","rule_major":0,"rule_minor":6,"proto_ver":5,"auth_hash":"{}","side":"sente"}}"#,
                "ab".repeat(32)
            )
        );

        let reveal = WireMessage::Reveal {
            action: "7g7f".to_string(),
            nonce: "11".repeat(32),
            board_hash: "22".repeat(32),
        };
        assert_eq!(
            reveal.to_json(),
            format!(
                r#"{{"type":"reveal","action":"7g7f","nonce":"{}","board_hash":"{}"}}"#,
                "11".repeat(32),
                "22".repeat(32)
            )
        );

        let reconnect_ack = WireMessage::ReconnectAck {
            board_hash: "33".repeat(32),
        };
        assert_eq!(
            reconnect_ack.to_json(),
            format!(r#"{{"type":"reconnect_ack","board_hash":"{}"}}"#, "33".repeat(32))
        );
    }

    #[test]
    fn unknown_type_rejected() {
        assert_eq!(
            WireMessage::from_json(r#"{"type":"peer_joined"}"#),
            Err(WireError::UnknownType)
        );
    }

    #[test]
    fn invalid_json_rejected() {
        assert_eq!(WireMessage::from_json("not json"), Err(WireError::InvalidJson));
    }

    #[test]
    fn hex_helpers_roundtrip() {
        let bytes = [0xabu8; 32];
        let hex = to_hex(&bytes);
        assert_eq!(hex.len(), 64);
        assert_eq!(from_hex32(&hex), Some(bytes));
        assert_eq!(from_hex32("short"), None);
        assert_eq!(from_hex32(&"zz".repeat(32)), None);
    }
}
