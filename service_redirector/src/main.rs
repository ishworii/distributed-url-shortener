pub mod state;

use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Redirect},
    routing::get,
};
use deadpool_redis::redis::AsyncCommands;
use state::{AppState, SharedState, create_pg_pool, create_redis_pool};
use std::sync::Arc;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://redis/".to_string());
    let db_url = std::env::var("DB_READ_URL")
        .unwrap_or_else(|_| "postgresql://user:password@db_master:5432/urls".to_string());
    let redis_ttl_seconds: u64 = std::env::var("REDIS_CACHE_TTL")
        .unwrap_or_else(|_| "604800".to_string())
        .parse()
        .unwrap_or(604800);
    let redis_pool = create_redis_pool(&redis_url)?;
    let db_read_pool = create_pg_pool(&db_url)?;

    let state = Arc::new(AppState {
        redis_pool,
        db_read_pool,
        redis_ttl_seconds,
    });
    let router = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/{key}", get(redirect_handler))
        .with_state(state);

    let listener = TcpListener::bind("0.0.0.0:8000").await?;
    println!("Rust redirection service listening on port 8000");
    axum::serve(listener, router.into_make_service()).await?;
    Ok(())
}

async fn redirect_handler(
    Path(short_key): Path<String>,
    State(state): State<SharedState>,
) -> Result<impl IntoResponse, StatusCode> {
    let mut redis_conn = state
        .redis_pool
        .get()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let cached_url: Option<String> = redis_conn
        .get(&short_key)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(url) = cached_url {
        return Ok(Redirect::temporary(&url));
    }
    let db_client = state
        .db_read_pool
        .get()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let row = db_client
        .query_opt(
            "SELECT long_url FROM url_mapping WHERE short_key=$1",
            &[&short_key],
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let long_url: String = row
        .try_get("long_url")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let _: () = redis_conn
        .set_ex(&short_key, &long_url, state.redis_ttl_seconds)
        .await
        .unwrap_or(());

    Ok(Redirect::temporary(&long_url))
}
