use anyhow::Result;
use gsa_local::app::App;

#[tokio::main]
async fn main() -> Result<()> {
    let mut app = App::new()?;
    app.run().await
}
