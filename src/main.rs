use tracing_subscriber::EnvFilter;

// Deux threads suffisent largement pour une maison, et gardent la mémoire basse sur un Pi Zero 2 W.
#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("heim=info,warn")),
        )
        .with_target(false)
        .init();

    let config = heim::Config::from_env()?;
    heim::run(config).await
}
