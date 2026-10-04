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
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        return admin(&args).await;
    }
    let cfg = Config::from_env()?;
    let db = PgPoolOptions::new()
        .max_connections(10)
        // Fail fast (503 + Retry-After) instead of queueing requests for 30 s.
        .acquire_timeout(std::time::Duration::from_secs(5))
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
    tokio::spawn(fraglink_server::games::refresh_loop(state.clone()));
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

/// Admin commands (run with the server's environment, e.g. on the VPS):
///   fraglink-server suggestions             list "Track this as a game?" answers
///   fraglink-server approve-game <exe> <name>   publish a program under a name
///   fraglink-server unapprove-game <exe>        show it as "Playing a game" again
async fn admin(args: &[String]) -> anyhow::Result<()> {
    let url =
        std::env::var("DATABASE_URL").map_err(|_| anyhow::anyhow!("DATABASE_URL is not set"))?;
    let db = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await?;
    MIGRATOR.run(&db).await?;
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["suggestions"] => {
            let rows: Vec<(String, String, i32, String, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
                "SELECT exe, name, reports, status, last_at FROM game_suggestions ORDER BY status = 'new' DESC, reports DESC, last_at DESC LIMIT 200",
            )
            .fetch_all(&db)
            .await?;
            for (exe, name, n, status, at) in rows {
                println!("{status:9} {n:4}  {exe:40} {name}  (last {})", at.format("%Y-%m-%d"));
            }
        }
        ["approve-game", exe, name @ ..] if !name.is_empty() => {
            let id = fraglink_server::names::approve(&db, exe, &name.join(" ")).await?;
            println!("approved {id}; shown under this name from now on");
        }
        ["unapprove-game", exe] => {
            let n = fraglink_server::names::unapprove(&db, exe).await?;
            println!("updated {n} game(s)");
        }
        _ => anyhow::bail!("usage: fraglink-server [suggestions | approve-game <exe> <name> | unapprove-game <exe>]"),
    }
    Ok(())
}
