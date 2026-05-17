CREATE TABLE nodes (
    id BIGSERIAL PRIMARY KEY,
    labels TEXT[] NOT NULL DEFAULT '{}'::TEXT[],
    properties JSONB NOT NULL DEFAULT '{}'::JSONB
);

CREATE TABLE relationships (
    id BIGSERIAL PRIMARY KEY,
    type TEXT NOT NULL,
    start_id BIGINT NOT NULL,
    end_id BIGINT NOT NULL,
    properties JSONB NOT NULL DEFAULT '{}'::JSONB
);

CREATE INDEX nodes_labels_gin_idx ON nodes USING GIN (labels);
CREATE INDEX relationships_type_idx ON relationships (type);
CREATE INDEX relationships_start_id_idx ON relationships (start_id);
CREATE INDEX relationships_end_id_idx ON relationships (end_id);
