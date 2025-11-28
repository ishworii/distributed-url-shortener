# TinyURL Service

A distributed URL shortening service built with a microservices architecture. This project demonstrates practical approaches to building scalable systems that handle high read/write loads efficiently.

## Architecture Overview

The system is composed of five main services:

1. **Key Generation Service (KGS)** - Python (background worker)
2. **URL Shortener Service** - Python/FastAPI
3. **URL Redirector Service** - Rust/Axum
4. **Analytics Worker** - Python (Kafka consumer)
5. **Supporting Infrastructure** - PostgreSQL, Redis, Kafka

Each service is containerized and communicates through a shared Docker network.

## System Design Decisions

### Key Generation Service (KGS)

The KGS is responsible for pre-generating unique short URL keys. This is a critical design choice that solves several problems:

**Why pre-generate keys?**
- Eliminates race conditions when multiple instances try to generate keys simultaneously
- Removes the need for distributed locks or coordination between shortener instances
- Provides consistent performance regardless of load
- Allows the shortener service to focus solely on URL mapping

**How it works:**
- Maintains a global sequence counter in PostgreSQL
- Reserves blocks of 1000 IDs at a time using atomic database operations
- Encodes numeric IDs into base62 strings (a-zA-Z0-9) for compact, URL-safe keys
- Pushes generated keys into a Redis set for instant access
- Continuously monitors the Redis key pool and refills when it drops below 20% capacity

This approach trades some storage space (pre-generated keys) for significant gains in performance and simplicity. The service runs as a background process, decoupled from user requests.

### URL Shortener Service

The shortener handles incoming requests to create short URLs. Key design choices:

**Database-first approach:**
- All URL mappings are persisted to PostgreSQL immediately
- Provides durability and serves as the source of truth
- Enables URL deduplication - if the same long URL is submitted twice, we return the existing short URL

**Redis as a write-through cache:**
- After creating a mapping, we cache it in Redis with a 7-day TTL
- Reduces database load for recently created URLs
- The cache is populated at write time, not lazily

**Why check for existing URLs?**
- Prevents key exhaustion from duplicate submissions
- Improves user experience by returning consistent short URLs for the same destination
- Reduces storage requirements

The service uses asyncpg for high-performance asynchronous PostgreSQL operations and connection pooling to handle concurrent requests efficiently.

### URL Redirector Service

The redirector handles the high-volume read path. This is where most of the traffic hits in a URL shortening service.

**Why Rust?**
- Superior performance for I/O-bound operations
- Low memory footprint
- Excellent concurrency handling through Tokio's async runtime
- Type safety prevents entire classes of runtime errors

**Two-tier caching strategy:**
1. Check Redis first (sub-millisecond lookup)
2. Fall back to PostgreSQL if not cached
3. Populate Redis with the result for future requests

This lazy caching approach means only popular URLs consume cache space, while the database handles long-tail traffic. The 7-day TTL ensures cache stays relevant without growing unbounded.

**Separation from shortener:**
- Allows independent scaling - you can run many redirector instances without affecting write throughput
- Different performance profiles - writes need strong consistency, reads prioritize speed
- Enables language/framework choices optimized for each workload

**Click event tracking:**
- Every redirect asynchronously publishes a click event to Kafka
- Non-blocking - doesn't slow down the redirect response
- Decouples analytics from the critical read path

### Analytics Service

The analytics worker provides real-time click tracking without impacting redirect performance.

**Event-driven architecture:**
- Redirector publishes click events to Kafka (fire-and-forget)
- Analytics worker consumes events asynchronously
- Events include short key and timestamp
- Stored in PostgreSQL for querying and analysis

**Why Kafka?**
- Handles massive event throughput with minimal latency
- Provides durability - events aren't lost even if analytics worker is down
- Enables future expansion - multiple consumers can process the same events for different purposes
- Decouples analytics from core redirect functionality

**Scalability:**
- Multiple analytics workers can consume in parallel (Kafka consumer groups)
- Batch processing can be added without changing the redirector
- Event stream can feed into data warehouses or analytics platforms

### Data Storage

**PostgreSQL:**
- ACID guarantees ensure URL mappings are never lost
- The `url_mapping` table uses the short key as primary key for fast lookups
- An index on `long_url` enables the duplicate detection query
- The `global_sequence` table provides atomic ID generation for KGS
- The `click_logs` table stores all click events for analytics (indexed on short_key)

**Redis:**
- Set data structure for the key pool (SPOP provides atomic retrieval)
- String keys for URL caching (simple key-value lookups)
- Configured with appropriate TTLs to prevent unbounded growth

**Kafka:**
- Topic `click_events` stores redirect events
- Consumer group `analytics-workers` for parallel processing
- KRaft mode (no Zookeeper dependency) for simpler deployment
- Events are retained to allow replay and recovery

## Configuration

All service configuration is managed through environment variables defined in `.env`:

- **Database credentials and connection strings**
- **Redis host and port**
- **KGS_BLOCK_SIZE**: Number of keys generated per batch (default: 1000)
- **REDIS_KEY_SET**: Redis set name for available keys
- **REDIS_CACHE_TTL**: Cache expiration time in seconds (default: 604800 = 7 days)
- **SHORT_URL_DOMAIN**: Base URL returned to users (use `http://localhost:8001` for local development)

This approach keeps service code clean and enables different configurations for development, staging, and production without code changes.

## Getting Started

### Prerequisites

- Docker and Docker Compose
- curl (for testing)

### Running the Service

1. Start all services:
```bash
docker-compose up -d
```

2. Verify services are running:
```bash
docker-compose ps
```

You should see all services healthy:
- `tinyurl_postgres` (port 5432)
- `tinyurl_redis` (port 6379)
- `tinyurl_kafka` (port 9092)
- `tinyurl_kgs`
- `tinyurl_shortener` (port 8000)
- `tinyurl_redirector` (port 8001)
- `tinyurl_analytics_worker`

3. Check KGS is generating keys:
```bash
docker-compose logs -f kgs
```

You should see output like:
```
Current available keys in Redis : 1000
Successfully generated and pushed 1000 keys: 1000 to 2000
```

### Testing the Service

**Create a short URL:**
```bash
curl -X POST http://localhost:8000/shorten \
  -H "Content-Type: application/json" \
  -d '{"long_url": "https://www.example.com"}'
```

Response:
```json
{"short_url":"http://localhost:8001/00000lg"}
```

**Test the redirect:**
```bash
curl -I http://localhost:8001/00000lg
```

You should see a `307 Temporary Redirect` with the location header pointing to your original URL.

**Open in browser:**
Simply paste `http://localhost:8001/00000lg` in your browser and you'll be redirected.

**Check analytics:**
```bash
docker exec tinyurl_postgres psql -U user -d urls -c "SELECT * FROM click_logs ORDER BY click_timestamp DESC LIMIT 10;"
```

You'll see all the redirects tracked with timestamps.

### Stopping the Service

```bash
docker-compose down
```

## Service Dependencies

The docker-compose configuration includes health checks and dependency ordering:

- Redis and PostgreSQL must be healthy before application services start
- Kafka starts independently (no health check as it self-initializes)
- KGS depends on both databases
- Shortener depends on databases and KGS
- Redirector depends on databases, Kafka, and shortener
- Analytics worker depends on databases and Kafka

This prevents startup race conditions where services try to connect before dependencies are ready.

## Scaling Considerations

**What scales horizontally:**
- URL Redirector - Run as many instances as needed behind a load balancer
- URL Shortener - Multiple instances can run concurrently (KGS eliminates coordination overhead)

**What should remain single-instance:**
- KGS - Multiple instances would work but add complexity. The single instance can generate millions of keys per minute, far exceeding typical needs.

**Bottlenecks to monitor:**
- PostgreSQL write throughput (shortener service)
- PostgreSQL read throughput (redirector cache misses)
- Redis memory usage (cache size)
- KGS key generation rate (should stay well ahead of shortener consumption)

## Future Enhancements

**Enhanced Analytics:**
- Aggregate click counts per URL
- Geographic distribution of requests (from IP addresses)
- Referrer information from HTTP headers
- Time-series analysis and trending URLs
- Real-time dashboards

**Expiration:**
- Allow users to set TTL on short URLs
- Automatically clean up expired mappings

**Custom short codes:**
- Let users specify their preferred short code (with availability checking)

**Rate limiting:**
- Prevent abuse of the shortening service
- Per-IP or per-user quotas

**API authentication:**
- Require API keys for shortening URLs
- Track usage per API key

## Project Structure

```
.
├── docker-compose.yml          # Service orchestration
├── .env                        # Configuration variables
├── scripts/
│   └── init_db.sql            # Database schema initialization
├── service_kgs/               # Key Generation Service
│   ├── Dockerfile
│   ├── requirements.txt
│   └── kgs_app/
│       └── core_logic.py      # Key generation logic
├── service_shortener/         # URL Shortener Service
│   ├── Dockerfile
│   ├── requirements.txt
│   └── shortener_app/
│       └── api.py             # Shortening API endpoints
├── service_redirector/        # URL Redirector Service
│   ├── Dockerfile
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs            # Redirect handler and Kafka producer
│       └── state.rs           # Application state and pooling
└── service_analytics/         # Analytics Worker Service
    ├── Dockerfile
    ├── requirements.txt
    └── worker_app/
        └── kafka_consumer.py  # Kafka consumer and DB writer
```

## Technical Notes

**Base62 encoding:**
The system uses base62 (a-z, A-Z, 0-9) to encode numeric IDs into compact strings. A 7-character base62 string can represent over 3.5 trillion unique URLs, more than sufficient for most applications.

**Connection pooling:**
Both Python services use asyncpg with connection pools (5-20 connections). The Rust service uses deadpool for both PostgreSQL (16 connections) and Redis. This balances resource usage with concurrent request handling.

**Database initialization:**
The PostgreSQL container runs `init_db.sql` on first startup, creating the necessary tables and indexes. The `global_sequence` table is seeded with an initial value to start key generation.

**Health checks:**
Docker health checks ensure services are truly ready before dependents start. PostgreSQL uses `pg_isready`, Redis uses `redis-cli ping`.

## License

This is a learning project demonstrating system design principles for building scalable web services.
