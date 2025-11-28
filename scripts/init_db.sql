--1 Table for KGS
CREATE TABLE IF NOT EXISTS global_sequence(
    id BIGINT PRIMARY KEY,
    next_val BIGINT NOT NULL
);

--2 Initialize the counter if it doesnt exist
INSERT INTO global_sequence(id,next_val)
VALUES (1,100)
ON CONFLICT (id) DO NOTHING;

--3 Table for URL Mappings
CREATE TABLE IF NOT EXISTS url_mapping(
    short_key VARCHAR(7) PRIMARY KEY,
    long_url TEXT NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP);

--Create a separate table for raw click logs
CREATE TABLE IF NOT EXISTS click_logs (
    id SERIAL PRIMARY KEY,
    short_key VARCHAR(7) NOT NULL,
    long_url TEXT, -- Can be retrieved later or included in event
    click_timestamp TIMESTAMP WITH TIME ZONE
);
-- Indexing on time for analytics
CREATE INDEX IF NOT EXISTS idx_click_ts ON click_logs (click_timestamp);

--4 create index
CREATE INDEX IF NOT EXISTS idx_long_url ON url_mapping(long_url);