#![cfg(feature = "server")]
#[test]
fn sql_is_confined_to_the_servers_own_schema() {
    veoveo_hub_sdk::testing::assert_sql_confined(env!("CARGO_MANIFEST_DIR"), "embedding");
}
