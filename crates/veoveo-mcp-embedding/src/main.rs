#[tokio::main]
async fn main() -> anyhow::Result<()> {
    veoveo_mcp_embedding::server::run().await
}
