import json
import os
import time

import psycopg2
from kafka import KafkaConsumer

KAFKA_BROKERS = os.environ.get("KAFKA_BROKERS", "kafka:9092").split(
    ","
)
ANALYTICS_DB_URL = os.environ.get(
    "ANALYTICS_DB_URL",
    "postgresql://user:password@db_master:5432/urls",
)
CLICK_TOPIC = os.environ.get("CLICK_TOPIC", "click_events")
GROUP_ID = os.environ.get("GROUP_ID", "analytics-workers")


def connect_db():
    conn = psycopg2.connect(ANALYTICS_DB_URL)
    conn.autocommit = True
    return conn


def run_consumer():
    print(f"Connecting to database: {ANALYTICS_DB_URL}")
    db_conn = connect_db()
    cursor = db_conn.cursor()
    print("Database connected successfully")

    print(f"Connecting to Kafka brokers: {KAFKA_BROKERS}")
    print(f"Topic: {CLICK_TOPIC}, Group: {GROUP_ID}")

    consumer = KafkaConsumer(
        CLICK_TOPIC,
        bootstrap_servers=KAFKA_BROKERS,
        group_id=GROUP_ID,
        auto_offset_reset="earliest",
        enable_auto_commit=True,
        value_deserializer=lambda m: json.loads(m.decode("utf-8")),
    )

    print(f"Kafka consumer connected successfully! Waiting for messages...")

    for message in consumer:
        print(f"Received message from Kafka: {message}")
        event = message.value

        short_key = event.get("key")
        timestamp_sec = event.get("timestamp")

        if not short_key or not timestamp_sec:
            print(f"Skipping malformed message : {event}")
            continue
        click_time = time.strftime(
            "%Y-%m-%d %H-%M:%S", time.gmtime(timestamp_sec)
        )
        try:
            insert_query = """
            INSERT INTO click_logs(short_key,click_timestamp)
            VALUES (%s,%s);
            """
            cursor.execute(insert_query, (short_key, click_time))
            print(f"Successfully inserted click event: key={short_key}, time={click_time}")
        except Exception as e:
            print(f"Error writing to DB:{e},event:{event}")


if __name__ == "__main__":
    print("Starting analytics worker...")
    while True:
        try:
            run_consumer()
        except Exception as e:
            print(f"Consumer failed,restarting in 5s: {e}")
            import traceback
            traceback.print_exc()
            time.sleep(5)
