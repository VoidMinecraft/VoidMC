// `DEP_VOIDMC_DATA_*` is exported by void-data/build.rs via `cargo::metadata`.

use std::env;
use std::fs;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/lib.rs");

    let lib = fs::read_to_string("src/lib.rs").expect("read src/lib.rs");
    let minecraft_version = const_value(&lib, "pub const MINECRAFT_VERSION: &str = \"", '"');
    let protocol_version: i64 = const_value(&lib, "pub const PROTOCOL_VERSION: i32 = ", ';')
        .parse()
        .expect("PROTOCOL_VERSION must be an integer literal");

    let shipped = env::var("DEP_VOIDMC_DATA_VERSIONS")
        .expect("voidmc-data did not export DEP_VOIDMC_DATA_VERSIONS (is `links` set?)");
    let shipped: Vec<&str> = shipped.split(',').collect();
    assert!(
        shipped.contains(&minecraft_version.as_str()),
        "voidmc-protocol targets Minecraft {minecraft_version} but voidmc-data ships only [{}]; \
         run void-data/scripts/extract.sh {minecraft_version} <jar> or fix MINECRAFT_VERSION",
        shipped.join(", ")
    );

    let key = format!(
        "DEP_VOIDMC_DATA_PROTOCOL_{}",
        minecraft_version.replace('.', "_")
    );
    let data_protocol: i64 = env::var(&key)
        .unwrap_or_else(|_| panic!("voidmc-data did not export {key}"))
        .parse()
        .expect("protocol metadata must be an integer");
    assert_eq!(
        protocol_version, data_protocol,
        "PROTOCOL_VERSION = {protocol_version} but void-data/assets/{minecraft_version}/version.json \
         says protocol_version = {data_protocol}; one of them is wrong"
    );
}

fn const_value(src: &str, prefix: &str, end: char) -> String {
    let start = src
        .find(prefix)
        .unwrap_or_else(|| panic!("src/lib.rs must contain `{prefix}...`"))
        + prefix.len();
    let rest = &src[start..];
    let len = rest
        .find(end)
        .unwrap_or_else(|| panic!("unterminated constant after `{prefix}`"));
    rest[..len].trim().to_string()
}
