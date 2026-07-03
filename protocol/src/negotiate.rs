/// 接続時バージョン交渉（純粋論理）。
///
/// タイムアウトの計時・メッセージ送受信は殻（net.rs）が担い、
/// このモジュールは「応答が来たか／不正か／一致するか」の判定だけを担う。

/// 版のタプル。対戦互換性は (ルール版, プロトコル版) の完全一致で決まる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionTuple {
    /// ルール仕様の版 (major, minor)
    pub rule: (u32, u32),
    /// プロトコルの版
    pub protocol: u32,
}

/// 版交渉が成功したことを型で保証するマーカー。
/// `negotiate_versions` が `Ok` を返したときにのみ取得できる。
/// このトークンがなければ認証・対局フェーズへ進めない（呼び出し側の規約）。
#[derive(Debug)]
pub struct VersionCleared;

/// 相手からの応答（殻が分類して渡す）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerVersionResponse {
    /// 版タプルとして正常に受信
    Version(VersionTuple),
    /// 応答が版タプルとして解釈できない（フォーマット不正・予期しないメッセージ）
    Invalid,
    /// 応答なし（殻側でタイムアウトを検出し、この値として渡す）
    Timeout,
}

/// 版交渉の失敗理由（成功は `Ok(VersionCleared)` で表現するためここに含まない）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NegotiationOutcome {
    Incompatible {
        mine: VersionTuple,
        theirs: VersionTuple,
        /// ルール版が不一致
        rule_mismatch: bool,
        /// プロトコル版が不一致
        protocol_mismatch: bool,
    },
    /// 相手の応答が版タプルとして解釈できない
    InvalidResponse,
    /// 相手が応答しなかった（v0.5.0 等、版交渉非対応の相手を含む過渡期の想定）
    Timeout,
}

/// このクレートが実装するプロトコルの版。
/// v0.6.0 でバージョン交渉が導入（版 = 1）。
/// v0.7.0 で投了（resign）を commit-reveal フローに追加（版 = 2）。
/// v0.10.0 で観戦系（spectate_meta/turn/result/status/init/token）と
/// DO の routing 拡張が加わり、ワイヤ表面が広がったため加算的に上げた（版 = 3）。
/// 対局チャネル（commit/reveal/ack/hello）自体はバイト不変。
pub const PROTOCOL_VERSION: u32 = 3;

/// 自分の版タプル（engine::RULE_VERSION + PROTOCOL_VERSION）
pub const MY_VERSION: VersionTuple = VersionTuple {
    rule: engine::RULE_VERSION,
    protocol: PROTOCOL_VERSION,
};

/// 版交渉の核心判定（純粋関数）。
///
/// - `Ok(VersionCleared)` — 互換。呼び出し側は次の認証フェーズへ進んでよい。
/// - `Err(NegotiationOutcome)` — 非互換・不正・タイムアウト。対戦不可。
pub fn negotiate_versions(
    mine: &VersionTuple,
    peer: PeerVersionResponse,
) -> Result<VersionCleared, NegotiationOutcome> {
    match peer {
        PeerVersionResponse::Version(theirs) => {
            let rule_mismatch     = mine.rule     != theirs.rule;
            let protocol_mismatch = mine.protocol != theirs.protocol;
            if rule_mismatch || protocol_mismatch {
                Err(NegotiationOutcome::Incompatible {
                    mine: mine.clone(),
                    theirs,
                    rule_mismatch,
                    protocol_mismatch,
                })
            } else {
                Ok(VersionCleared)
            }
        }
        PeerVersionResponse::Invalid => Err(NegotiationOutcome::InvalidResponse),
        PeerVersionResponse::Timeout => Err(NegotiationOutcome::Timeout),
    }
}

// ─── テスト ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn v(rule_major: u32, rule_minor: u32, protocol: u32) -> VersionTuple {
        VersionTuple { rule: (rule_major, rule_minor), protocol }
    }

    // §7 テスト1: 同一タプル → 互換
    #[test]
    fn compatible_same_tuple() {
        let mine = v(0, 5, 1);
        let result = negotiate_versions(&mine, PeerVersionResponse::Version(mine.clone()));
        assert!(result.is_ok(), "同一タプルは互換のはず");
    }

    // §7 テスト2: ルール版のみ不一致 → 非互換（rule_mismatch=true, protocol_mismatch=false）
    #[test]
    fn incompatible_rule_only() {
        let mine  = v(0, 5, 1);
        let theirs = v(0, 6, 1);
        match negotiate_versions(&mine, PeerVersionResponse::Version(theirs)) {
            Err(NegotiationOutcome::Incompatible { rule_mismatch, protocol_mismatch, .. }) => {
                assert!(rule_mismatch,      "ルール版不一致のはず");
                assert!(!protocol_mismatch, "プロトコル版は一致のはず");
            }
            other => panic!("Incompatible を期待したが {:?}", other),
        }
    }

    // §7 テスト3: プロトコル版のみ不一致 → 非互換（rule_mismatch=false, protocol_mismatch=true）
    #[test]
    fn incompatible_protocol_only() {
        let mine  = v(0, 5, 1);
        let theirs = v(0, 5, 2);
        match negotiate_versions(&mine, PeerVersionResponse::Version(theirs)) {
            Err(NegotiationOutcome::Incompatible { rule_mismatch, protocol_mismatch, .. }) => {
                assert!(!rule_mismatch,    "ルール版は一致のはず");
                assert!(protocol_mismatch, "プロトコル版不一致のはず");
            }
            other => panic!("Incompatible を期待したが {:?}", other),
        }
    }

    // §7 テスト4: 両方不一致 → 非互換（両フラグ true）
    #[test]
    fn incompatible_both() {
        let mine  = v(0, 5, 1);
        let theirs = v(0, 6, 2);
        match negotiate_versions(&mine, PeerVersionResponse::Version(theirs)) {
            Err(NegotiationOutcome::Incompatible { rule_mismatch, protocol_mismatch, .. }) => {
                assert!(rule_mismatch,     "ルール版不一致のはず");
                assert!(protocol_mismatch, "プロトコル版不一致のはず");
            }
            other => panic!("Incompatible を期待したが {:?}", other),
        }
    }

    // §7 テスト5: 非互換の返り値に両者の版が含まれる（更新案内・寄せ先は含まない）
    #[test]
    fn incompatible_contains_both_versions() {
        let mine  = v(0, 5, 1);
        let theirs = v(0, 6, 2);
        match negotiate_versions(&mine, PeerVersionResponse::Version(theirs.clone())) {
            Err(NegotiationOutcome::Incompatible { mine: m, theirs: t, .. }) => {
                assert_eq!(m, mine,   "自分の版が含まれるはず");
                assert_eq!(t, theirs, "相手の版が含まれるはず");
                // 「どちらに更新せよ」等の案内は含まない（構造体に持たない）
            }
            other => panic!("Incompatible を期待したが {:?}", other),
        }
    }

    // §7 テスト6: 不正応答 → InvalidResponse
    #[test]
    fn invalid_response() {
        let mine = v(0, 5, 1);
        let result = negotiate_versions(&mine, PeerVersionResponse::Invalid);
        assert!(matches!(result, Err(NegotiationOutcome::InvalidResponse)));
    }

    // §7 テスト7: 応答なし（タイムアウト相当）→ Timeout
    #[test]
    fn timeout_response() {
        let mine = v(0, 5, 1);
        let result = negotiate_versions(&mine, PeerVersionResponse::Timeout);
        assert!(matches!(result, Err(NegotiationOutcome::Timeout)));
    }

    // §7 テスト8: 順序保証 — VersionCleared は Ok からしか得られない
    // 互換時のみ Ok(VersionCleared) が返り、認証フェーズへ進める。
    // 非互換・不正・タイムアウト時は Err になり VersionCleared を得られない。
    #[test]
    fn version_cleared_only_from_compatible() {
        let mine = v(0, 5, 1);

        // 互換 → VersionCleared を取得できる
        let cleared = negotiate_versions(&mine, PeerVersionResponse::Version(mine.clone()));
        assert!(cleared.is_ok(), "互換時は Ok(VersionCleared) のはず");

        // 非互換 → VersionCleared を取得できない（Err）
        assert!(negotiate_versions(&mine, PeerVersionResponse::Version(v(0, 6, 2))).is_err());
        assert!(negotiate_versions(&mine, PeerVersionResponse::Invalid).is_err());
        assert!(negotiate_versions(&mine, PeerVersionResponse::Timeout).is_err());
    }
}
