use engine::types::Action;
use sha2::{Digest, Sha256};

/// SHA-256(action_usi || nonce)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Commitment(pub [u8; 32]);

/// 乱数ノンス（殻側が生成して注入する）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nonce(pub [u8; 32]);

pub fn make_commit(action: Action, nonce: &Nonce) -> Commitment {
    let mut h = Sha256::new();
    h.update(action.to_usi().as_bytes());
    h.update(nonce.0);
    Commitment(h.finalize().into())
}

pub fn verify_commit(commitment: &Commitment, action: Action, nonce: &Nonce) -> bool {
    make_commit(action, nonce) == *commitment
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::types::Action;

    fn mv(s: &str) -> Action {
        Action::from_usi(s).unwrap()
    }

    /// 拘束性: 異なる着手では commit が開けない
    #[test]
    fn binding_wrong_action_fails() {
        let action_a = mv("7g7f");
        let action_b = mv("3c3d");
        let nonce = Nonce([1u8; 32]);
        let commit = make_commit(action_a, &nonce);
        assert!(
            !verify_commit(&commit, action_b, &nonce),
            "wrong action must fail"
        );
        assert!(
            verify_commit(&commit, action_a, &nonce),
            "correct action must pass"
        );
    }

    /// 拘束性: 正しい着手でも nonce が違えば失敗
    #[test]
    fn binding_wrong_nonce_fails() {
        let action = mv("7g7f");
        let nonce_a = Nonce([1u8; 32]);
        let nonce_b = Nonce([2u8; 32]);
        let commit = make_commit(action, &nonce_a);
        assert!(!verify_commit(&commit, action, &nonce_b));
    }

    /// 秘匿性: 同じ着手でも nonce が違えば commit は異なる
    #[test]
    fn hiding_different_nonces_produce_different_commits() {
        let action = mv("7g7f");
        let nonce_a = Nonce([0u8; 32]);
        let nonce_b = Nonce([1u8; 32]);
        let ca = make_commit(action, &nonce_a);
        let cb = make_commit(action, &nonce_b);
        assert_ne!(ca.0, cb.0);
    }

    /// 秘匿性: 異なる着手・同じ nonce でも commit は異なる
    #[test]
    fn hiding_different_actions_produce_different_commits() {
        let nonce = Nonce([42u8; 32]);
        let ca = make_commit(mv("7g7f"), &nonce);
        let cb = make_commit(mv("3c3d"), &nonce);
        assert_ne!(ca.0, cb.0);
    }
}
