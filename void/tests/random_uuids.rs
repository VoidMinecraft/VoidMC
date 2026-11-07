use std::process::Command;

const CHILD: &str = "VOIDMC_UUIDS_CHILD";
const NAME: &str = "a_server_that_is_not_stepped_draws_random_uuids";

fn first_uuid_of_a_new_process() -> String {
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", NAME, "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .find_map(|line| line.split_once("uuid=").map(|(_, id)| id[..36].to_owned()))
        .unwrap()
}

#[test]
fn a_server_that_is_not_stepped_draws_random_uuids() {
    if std::env::var_os(CHILD).is_some() {
        println!("uuid={}", voidmc::uuids::next());
        return;
    }
    assert_ne!(voidmc::uuids::next(), voidmc::uuids::next());
    assert_ne!(first_uuid_of_a_new_process(), first_uuid_of_a_new_process());
}
