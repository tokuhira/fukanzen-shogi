use sha2::{Digest, Sha256};

/// 共有秘密の SHA-256（対局前に相手へ渡すもの）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecretHash(pub [u8; 32]);

pub fn hash_secret(secret: &[u8]) -> SecretHash {
    let mut h = Sha256::new();
    h.update(secret);
    SecretHash(h.finalize().into())
}

/// 再接続時: 相手が提示した秘密の本体を検証
pub fn verify_secret(claimed: &[u8], expected: &SecretHash) -> bool {
    hash_secret(claimed) == *expected
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 本人認証: 正しい秘密は受理
    #[test]
    fn correct_secret_accepted() {
        let hash = hash_secret(b"shared_password");
        assert!(verify_secret(b"shared_password", &hash));
    }

    /// 本人認証: 誤った秘密はリジェクト
    #[test]
    fn wrong_secret_rejected() {
        let hash = hash_secret(b"shared_password");
        assert!(!verify_secret(b"wrong", &hash));
        assert!(!verify_secret(b"", &hash));
    }
}
