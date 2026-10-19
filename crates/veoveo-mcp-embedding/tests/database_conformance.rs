#![cfg(feature = "server")]
#[tokio::test]
async fn empty_domain_schema_and_native_core_queries_follow_database_rules() {
    veoveo_fixtures::database::Conformance::new()
        .server(
            &veoveo_mcp_embedding::server(),
            veoveo_mcp_embedding::migrator(),
        )
        .queries_from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.sqlx"))
        .sources(concat!(env!("CARGO_MANIFEST_DIR"), "/src"))
        .check()
        .await;
}
