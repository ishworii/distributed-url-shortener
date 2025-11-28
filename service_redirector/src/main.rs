pub mod state;

use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Redirect},
    routing::get,
};
use deadpool_redis::redis::AsyncCommands;
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::util::Timeout;

use state::{AppState, SharedState, create_pg_pool, create_redis_pool};
use std::{sync::Arc, time::Duration};
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

    let producer: FutureProducer = rdkafka::config::ClientConfig::new()
        .set("bootstrap.servers", "kafka:9092")
        .set("message.timeout.ms", "5000")
        .create()
        .expect("Producer creation error");

    let state = Arc::new(AppState {
        redis_pool,
        db_read_pool,
        redis_ttl_seconds,
        kafka_producer: producer,
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
    println!("Redirect request for key: {}", short_key);

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

    let key_clone = short_key.clone();
    let producer = state.kafka_producer.clone();
    tokio::spawn(async move {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let payload = format!(r#"{{"key":"{}","timestamp":{}}}"#, key_clone, timestamp);

        println!("Preparing to send click event for key: {}", key_clone);
        println!("Payload: {}", payload);

        let click_topic =
            std::env::var("CLICK_TOPIC").unwrap_or_else(|_| "click_events".to_string());
        println!("Sending to topic: {}", click_topic);

        let record = FutureRecord::to(&click_topic)
            .key(&key_clone)
            .payload(&payload);
        let kafka_timeout =
            std::env::var("KAFKA_TIMEOUT_MS").unwrap_or_else(|_| "5000".to_string());
        let kafka_timeout = kafka_timeout
            .parse()
            .expect("Failed to convert kafka timeout to u64");
        let queue_timeout = Timeout::After(Duration::from_millis(kafka_timeout));

        match producer.send(record, queue_timeout).await {
            Ok(_) => println!("Successfully sent click event to Kafka for key: {}", key_clone),
            Err((e, _)) => eprintln!("Failed to send click event to Kafka: {:?}", e),
        }
    });

    Ok(Redirect::temporary(&long_url))
}
