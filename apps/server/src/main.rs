use fraglink_server::{router, AppState, Config, MIGRATOR};
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn".into()),
        )
        .init();
    let cfg = Config::from_env()?;
    let db = PgPoolOptions::new()
        .max_connections(10)
        .connect(&cfg.database_url)
        .await?;
    MIGRATOR.run(&db).await?;
    if cfg.steam_api_key.is_none() {
        tracing::warn!("STEAM_API_KEY is not set: Steam import is disabled (linking still works)");
    }
    let bind = cfg.bind.clone();
    tracing::info!(
        "fraglink-server listening on http://{bind} (public URL {})",
        cfg.public_url
    );
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    let state = AppState::new(db, cfg);
    fraglink_server::presence::reset_all(&state)
        .await
        .map_err(|e| anyhow::anyhow!(e.message))?;
    let bg = state.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            tick.tick().await;
            match fraglink_server::steam::sync_due(&bg, 200).await {
                Ok(n) if n > 0 => tracing::info!("daily Steam sync: refreshed {n} account(s)"),
                Ok(_) => {}
                Err(e) => tracing::warn!("daily Steam sync failed: {}", e.message),
            }
        }
    });
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
