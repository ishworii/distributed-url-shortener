use deadpool_postgres::{
    Manager, ManagerConfig, Pool as PgPool, RecyclingMethod,
};
use deadpool_redis::Pool as RedisPool;
use std::sync::Arc;
use tokio_postgres::{Config, NoTls};

pub struct AppState {
    pub redis_pool: RedisPool,
    pub db_read_pool: PgPool,
    pub redis_ttl_seconds: u64,
}

pub type SharedState = Arc<AppState>;

pub fn create_pg_pool(db_url: &str) -> Result<PgPool, Box<dyn std::error::Error>> {
    let pg_config = db_url.parse::<Config>()?;
    let mgr_config = ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    };
    let mgr = Manager::from_config(pg_config, NoTls, mgr_config);
    let pool = PgPool::builder(mgr).max_size(16).build()?;

    Ok(pool)
}

pub fn create_redis_pool(redis_url :&str) -> Result<RedisPool,Box<dyn std::error::Error>>{
    let cfg = deadpool_redis::Config::from_url(redis_url);
    let pool = cfg.create_pool(Some(deadpool_redis::Runtime::Tokio1))?;
    Ok(pool)
}
