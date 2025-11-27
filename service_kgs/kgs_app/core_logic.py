import asyncio
import os

import asyncpg
import redis.asyncio as redis

BASE = 62
CHARACTERS = (
    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
)
BLOCK_SIZE = int(os.environ.get("KGS_BLOCK_SIZE", 1000))
REDIS_KEY_SET = os.environ.get("REDIS_KEY_SET", "available_keys")
KGS_DB_URL = os.environ.get("KGS_DB_URL", "postgresql://user:password@db_master:5432/urls")
REDIS_HOST = os.environ.get("REDIS_HOST", "redis")
REDIS_PORT = int(os.environ.get("REDIS_PORT", 6379))


def encode_base62(decimal_id):
    if decimal_id == 0:
        return CHARACTERS[0].zfill(7)

    encoded = []
    temp_id = decimal_id
    while temp_id > 0:
        remainder = temp_id % BASE
        encoded.append(CHARACTERS[remainder])
        temp_id //= BASE

    return "".join(reversed(encoded)).zfill(7)


async def reserve_and_generate_block(redis_client):
    conn = None
    try:
        conn = await asyncpg.connect(KGS_DB_URL)
        query = f"UPDATE global_sequence SET next_val = next_val + {BLOCK_SIZE} WHERE id=1 RETURNING next_val - {BLOCK_SIZE}"
        start_record = await conn.fetchrow(query)
        if not start_record:
            raise Exception("Failed to reserve ID block.")
        start_id = start_record[0]
        end_id = start_id + BLOCK_SIZE
        # generate keys
        keys = []
        for i in range(start_id, end_id):
            keys.append(encode_base62(i))
        # push to redis
        await redis_client.sadd(REDIS_KEY_SET, *keys)
        print(
            f"Successfully generated and pushed {BLOCK_SIZE} keys: {start_id} to {end_id}"
        )
    except Exception as e:
        print(f"Error in KGS : {e}")
    finally:
        if conn:
            await conn.close()


async def main():
    redis_client = redis.Redis(host=REDIS_HOST, port=REDIS_PORT)
    while True:
        key_count = await redis_client.scard(REDIS_KEY_SET)
        print(f"Current available keys in Redis : {key_count}")

        if key_count < BLOCK_SIZE * 0.2:
            await reserve_and_generate_block(redis_client)

        await asyncio.sleep(5)


if __name__ == "__main__":
    asyncio.run(main())
