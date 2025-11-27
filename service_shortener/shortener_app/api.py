import os
from contextlib import asynccontextmanager

import asyncpg
import redis.asyncio as redis
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel

DB_URL = os.environ.get(
    "URL_DB_MASTER_URL", "postgresql://user:pass@db_master:5432/urls"
)
REDIS_HOST = os.environ.get("REDIS_HOST", "redis")
REDIS_PORT = int(os.environ.get("REDIS_PORT", 6379))
REDIS_KEY_SET = "available_keys"


class URLRequest(BaseModel):
    long_url: str


class URLResponse(BaseModel):
    short_url: str


db_pool = None
redis_client = None


@asynccontextmanager
async def lifespan(app: FastAPI):
    global db_pool, redis_client
    try:
        db_pool = await asyncpg.create_pool(
            dsn=DB_URL, min_size=5, max_size=20
        )
        print("Successfully connected to db")
        redis_client = redis.Redis(host=REDIS_HOST, port=REDIS_PORT)
        await redis_client.ping()
        print("Successfully connected to redis.")
    except Exception as e:
        raise RuntimeError(
            "Failed to connect to required services.", e
        )
    yield
    print("Shutting down resources.")
    if db_pool:
        await db_pool.close()
    if redis_client:
        await redis_client.close()


app = FastAPI(title="TinyURL Shortener Service", lifespan=lifespan)


async def get_unique_key() -> str:
    key = await redis_client.spop(REDIS_KEY_SET)
    if key is None:
        raise HTTPException(
            status_code=503,
            detail="Key generation service is depleted. Please try again after a while.",
        )
    return key.decode("utf-8")


@app.post("/shorten", response_model=URLResponse)
async def shorten_url(request: URLRequest):
    long_url = request.long_url.strip()
    async with db_pool.acquire() as conn:
        existing_mapping = await conn.fetchrow(
            "SELECT short_key,long_url FROM url_mapping WHERE long_url = $1",
            long_url,
        )
        if existing_mapping:
            existing_key = existing_mapping["short_key"]
            await redis_client.set(
                existing_key, long_url, ex=60 * 60 * 24 * 7
            )
            return URLResponse(
                short_url=f"http://tiny.url/{existing_key}"
            )

    short_key = await get_unique_key()
    async with db_pool.acquire() as conn:
        try:
            await conn.execute(
                "INSERT INTO url_mapping (short_key,long_url) VALUES ($1,$2)",
                short_key,
                long_url,
            )
        except Exception as e:
            print(f"DB insert failed for key {short_key} : {e}")
            raise HTTPException(
                status_code=500, detail="Database write error."
            )
    await redis_client.set(short_key, long_url, ex=60 * 60 * 24 * 7)
    return URLResponse(short_url=f"http://tiny.url/{short_key}")


@app.get("/health")
def health_check():
    return {"status": "ok"}
