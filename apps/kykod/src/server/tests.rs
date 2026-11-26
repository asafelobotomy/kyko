use super::peer_allowed;

#[test]
fn only_the_same_user_may_connect() {
    assert!(peer_allowed(1000, 1000));
    assert!(!peer_allowed(1001, 1000));
    assert!(!peer_allowed(0, 1000));
}
