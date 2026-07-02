use crate::kifu::Kifu;
use crate::serialize::{kifu_to_string, kifu_from_string};

pub const ARCHIVE_FORMAT_VERSION: u32 = 1;
const MAGIC: &str = "fukanzen-shogi-archive";

#[derive(Debug, Clone, PartialEq)]
pub struct ArchiveMeta {
    pub rule: (u32, u32),
    pub protocol: u32,
    pub app: Option<String>,
    pub sente: Option<String>,
    pub gote: Option<String>,
    pub result: ArchiveResult,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArchiveResult {
    pub kind: ResultKind,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResultKind {
    Mate,
    KingDeath,
    SwapDraw,
    Sennichite,
    MaxTurns,
    Resign,
    Unfinished,
    Other,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    SenteWins,
    GoteWins,
    Draw,
    None,
}

impl ResultKind {
    pub fn to_str(&self) -> &'static str {
        match self {
            ResultKind::Mate       => "mate",
            ResultKind::KingDeath  => "king_death",
            ResultKind::SwapDraw   => "swap_draw",
            ResultKind::Sennichite => "sennichite",
            ResultKind::MaxTurns   => "max_turns",
            ResultKind::Resign     => "resign",
            ResultKind::Unfinished => "unfinished",
            ResultKind::Other      => "other",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "mate"       => Some(ResultKind::Mate),
            "king_death" => Some(ResultKind::KingDeath),
            "swap_draw"  => Some(ResultKind::SwapDraw),
            "sennichite" => Some(ResultKind::Sennichite),
            "max_turns"  => Some(ResultKind::MaxTurns),
            "resign"     => Some(ResultKind::Resign),
            "unfinished" => Some(ResultKind::Unfinished),
            "other"      => Some(ResultKind::Other),
            _            => None,
        }
    }
}

impl Outcome {
    pub fn to_str(&self) -> &'static str {
        match self {
            Outcome::SenteWins => "sente_wins",
            Outcome::GoteWins  => "gote_wins",
            Outcome::Draw      => "draw",
            Outcome::None      => "none",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "sente_wins" => Some(Outcome::SenteWins),
            "gote_wins"  => Some(Outcome::GoteWins),
            "draw"       => Some(Outcome::Draw),
            "none"       => Some(Outcome::None),
            _            => None,
        }
    }
}

/// アーカイブ書式 v1 で Kifu を文字列化する。
///
/// ヘッダ順: magic → rule → protocol → app → sente → gote → result → kifu 本文
pub fn kifu_to_archive(kifu: &Kifu, meta: &ArchiveMeta) -> String {
    let mut parts = Vec::new();
    parts.push(format!("{} {}", MAGIC, ARCHIVE_FORMAT_VERSION));
    parts.push(format!("rule {}.{}", meta.rule.0, meta.rule.1));
    parts.push(format!("protocol {}", meta.protocol));
    parts.push(format!("app {}", meta.app.as_deref().unwrap_or("-")));
    parts.push(format!("sente {}", meta.sente.as_deref().unwrap_or("-")));
    parts.push(format!("gote {}", meta.gote.as_deref().unwrap_or("-")));
    parts.push(format!(
        "result {} {}",
        meta.result.kind.to_str(),
        meta.result.outcome.to_str()
    ));
    parts.push(kifu_to_string(kifu));
    parts.join("\n")
}

/// アーカイブ文字列（v1 または旧 sfen 始まり）を Kifu と ArchiveMeta に変換する。
///
/// 旧書式（`sfen` で始まる行）は後方互換として受け付ける。
/// 未知のヘッダキーは無視する（前方互換）。
pub fn archive_to_kifu(s: &str) -> Option<(Kifu, ArchiveMeta)> {
    let lines: Vec<&str> = s.lines().collect();
    if lines.is_empty() {
        return None;
    }

    // 後方互換: 旧書式は "sfen " で始まる
    if lines[0].trim().starts_with("sfen ") {
        let kifu = kifu_from_string(s)?;
        let meta = ArchiveMeta {
            rule: crate::RULE_VERSION,
            protocol: 0,
            app: None,
            sente: None,
            gote: None,
            result: ArchiveResult {
                kind: ResultKind::Unfinished,
                outcome: Outcome::None,
            },
        };
        return Some((kifu, meta));
    }

    // マジック行の検証
    let (magic_word, _ver_str) = lines[0].trim().split_once(' ')?;
    if magic_word != MAGIC {
        return None;
    }

    // ヘッダをパース（sfen 行まで）
    let mut rule: Option<(u32, u32)> = None;
    let mut protocol: Option<u32> = None;
    let mut app: Option<String> = None;
    let mut sente: Option<String> = None;
    let mut gote: Option<String> = None;
    let mut result: Option<ArchiveResult> = None;
    let mut body_start: Option<usize> = None;

    for (i, line) in lines.iter().enumerate().skip(1) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("sfen ") {
            body_start = Some(i);
            break;
        }
        if let Some((key, value)) = line.split_once(' ') {
            match key {
                "rule" => {
                    if let Some((a, b)) = value.split_once('.') {
                        if let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>()) {
                            rule = Some((a, b));
                        }
                    }
                }
                "protocol" => {
                    protocol = value.parse().ok();
                }
                "app" => {
                    app = if value == "-" { None } else { Some(value.to_string()) };
                }
                "sente" => {
                    sente = if value == "-" { None } else { Some(value.to_string()) };
                }
                "gote" => {
                    gote = if value == "-" { None } else { Some(value.to_string()) };
                }
                "result" => {
                    if let Some((ks, os)) = value.split_once(' ') {
                        if let (Some(k), Some(o)) =
                            (ResultKind::from_str(ks), Outcome::from_str(os))
                        {
                            result = Some(ArchiveResult { kind: k, outcome: o });
                        }
                    }
                }
                _ => {} // 前方互換: 未知キーは無視
            }
        }
    }

    let body_start = body_start?;
    let kifu_body = lines[body_start..].join("\n");
    let kifu = kifu_from_string(&kifu_body)?;

    Some((
        kifu,
        ArchiveMeta {
            rule: rule?,
            protocol: protocol?,
            app,
            sente,
            gote,
            result: result?,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serialize::{INITIAL_SFEN, sfen_to_position};
    use crate::types::{Action, Ply, Square};

    fn initial_kifu() -> Kifu {
        Kifu::new(sfen_to_position(INITIAL_SFEN).unwrap())
    }

    fn test_ply() -> Ply {
        Ply {
            sente: Action::Move {
                from: Square::new(7, 7),
                to: Square::new(7, 6),
                promote: false,
            },
            gote: Action::Move {
                from: Square::new(3, 3),
                to: Square::new(3, 4),
                promote: false,
            },
        }
    }

    fn default_meta() -> ArchiveMeta {
        ArchiveMeta {
            rule: (0, 5),
            protocol: 2,
            app: Some("0.8.0".to_string()),
            sente: None,
            gote: None,
            result: ArchiveResult {
                kind: ResultKind::Unfinished,
                outcome: Outcome::None,
            },
        }
    }

    #[test]
    fn round_trip() {
        let mut kifu = initial_kifu();
        kifu.push(test_ply());
        let meta = default_meta();

        let archive = kifu_to_archive(&kifu, &meta);
        let (kifu2, meta2) = archive_to_kifu(&archive).expect("parse failed");

        assert_eq!(kifu2.plies.len(), kifu.plies.len());
        assert_eq!(kifu2.plies[0], kifu.plies[0]);
        assert_eq!(meta2.rule, meta.rule);
        assert_eq!(meta2.protocol, meta.protocol);
        assert_eq!(meta2.app, meta.app);
        assert_eq!(meta2.result, meta.result);
    }

    #[test]
    fn deterministic() {
        let kifu = initial_kifu();
        let meta = default_meta();
        let a1 = kifu_to_archive(&kifu, &meta);
        let a2 = kifu_to_archive(&kifu, &meta);
        assert_eq!(a1, a2);
    }

    #[test]
    fn header_order() {
        let kifu = initial_kifu();
        let meta = default_meta();
        let archive = kifu_to_archive(&kifu, &meta);
        let lines: Vec<&str> = archive.lines().collect();

        assert!(lines[0].starts_with("fukanzen-shogi-archive "));
        assert!(lines[1].starts_with("rule "));
        assert!(lines[2].starts_with("protocol "));
        assert!(lines[3].starts_with("app "));
        assert!(lines[4].starts_with("sente "));
        assert!(lines[5].starts_with("gote "));
        assert!(lines[6].starts_with("result "));
        assert!(lines[7].starts_with("sfen "));
    }

    #[test]
    fn backward_compat_old_kifu() {
        let old = "sfen lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1\n1: 7g7f | 3c3d";
        let (kifu, meta) = archive_to_kifu(old).expect("old format should parse");
        assert_eq!(kifu.plies.len(), 1);
        assert_eq!(meta.result.kind, ResultKind::Unfinished);
        assert_eq!(meta.result.outcome, Outcome::None);
    }

    #[test]
    fn forward_compat_unknown_header() {
        let archive = concat!(
            "fukanzen-shogi-archive 1\n",
            "rule 0.5\n",
            "protocol 2\n",
            "app -\n",
            "sente -\n",
            "gote -\n",
            "unknown_key some_value\n",
            "result unfinished none\n",
            "sfen lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1\n"
        );
        let (kifu, meta) = archive_to_kifu(archive).expect("unknown key should be ignored");
        assert_eq!(kifu.plies.len(), 0);
        assert_eq!(meta.rule, (0, 5));
    }

    #[test]
    fn result_round_trip() {
        let kinds = [
            ResultKind::Mate,
            ResultKind::KingDeath,
            ResultKind::SwapDraw,
            ResultKind::Sennichite,
            ResultKind::MaxTurns,
            ResultKind::Resign,
            ResultKind::Unfinished,
            ResultKind::Other,
        ];
        let outcomes = [
            Outcome::SenteWins,
            Outcome::GoteWins,
            Outcome::Draw,
            Outcome::None,
        ];
        for k in &kinds {
            assert_eq!(ResultKind::from_str(k.to_str()), Some(k.clone()), "{:?}", k);
        }
        for o in &outcomes {
            assert_eq!(Outcome::from_str(o.to_str()), Some(o.clone()), "{:?}", o);
        }
    }

    #[test]
    fn rule_parse() {
        let meta = ArchiveMeta {
            rule: (0, 5),
            protocol: 2,
            app: None,
            sente: None,
            gote: None,
            result: ArchiveResult {
                kind: ResultKind::Unfinished,
                outcome: Outcome::None,
            },
        };
        let kifu = initial_kifu();
        let archive = kifu_to_archive(&kifu, &meta);
        let (_, meta2) = archive_to_kifu(&archive).unwrap();
        assert_eq!(meta2.rule, (0, 5));
    }

    #[test]
    fn optional_fields_roundtrip() {
        let meta = ArchiveMeta {
            rule: (0, 5),
            protocol: 2,
            app: Some("0.8.0".to_string()),
            sente: Some("alice".to_string()),
            gote: Some("bob".to_string()),
            result: ArchiveResult {
                kind: ResultKind::Mate,
                outcome: Outcome::SenteWins,
            },
        };
        let kifu = initial_kifu();
        let archive = kifu_to_archive(&kifu, &meta);
        let (_, meta2) = archive_to_kifu(&archive).unwrap();
        assert_eq!(meta2.sente, Some("alice".to_string()));
        assert_eq!(meta2.gote, Some("bob".to_string()));
        assert_eq!(meta2.app, Some("0.8.0".to_string()));
        assert_eq!(meta2.result.kind, ResultKind::Mate);
        assert_eq!(meta2.result.outcome, Outcome::SenteWins);
    }
}
