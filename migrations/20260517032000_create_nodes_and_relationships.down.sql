DROP INDEX IF EXISTS relationships_end_id_idx;
DROP INDEX IF EXISTS relationships_start_id_idx;
DROP INDEX IF EXISTS relationships_type_idx;
DROP INDEX IF EXISTS nodes_labels_gin_idx;

DROP TABLE IF EXISTS relationships;
DROP TABLE IF EXISTS nodes;
