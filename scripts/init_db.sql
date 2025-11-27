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

--4 create index
CREATE INDEX IF NOT EXISTS idx_long_url ON url_mapping(long_url);