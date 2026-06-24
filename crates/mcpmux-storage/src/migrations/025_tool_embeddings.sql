CREATE TABLE IF NOT EXISTS tool_embeddings (
    content_hash TEXT NOT NULL,
    model_version TEXT NOT NULL,
    vector BLOB NOT NULL,
    dims INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (content_hash, model_version)
);
