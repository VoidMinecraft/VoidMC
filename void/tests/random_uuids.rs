#[test]
fn a_server_that_is_not_stepped_draws_random_uuids() {
    assert_ne!(voidmc::uuids::next(), voidmc::uuids::next());
}
