//! Process entry. Hub launches this binary; `server::run` owns the loopback page.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    zectrix_ai_daily::server::run().await
}
