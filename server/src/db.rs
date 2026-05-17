use crate::domain::{Node, Properties, Relationship, Value};
use serde_json::Value as JsonValue;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Json;
use sqlx::{PgPool, Postgres, Row, Transaction};

/// Builds the shared PostgreSQL connection pool.
pub async fn create_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await
}

#[derive(Debug, Clone)]
pub struct NodeRepo {
    pool: PgPool,
}

impl NodeRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn insert(
        &self,
        labels: Vec<String>,
        properties: Properties,
    ) -> Result<Node, sqlx::Error> {
        self.insert_in_txless(labels, properties).await
    }

    pub async fn insert_in_tx(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        labels: Vec<String>,
        properties: Properties,
    ) -> Result<Node, sqlx::Error> {
        let row = sqlx::query(
            r#"
            INSERT INTO nodes (labels, properties)
            VALUES ($1, $2)
            RETURNING id, labels, properties
            "#,
        )
        .bind(labels)
        .bind(Json(properties))
        .fetch_one(&mut **transaction)
        .await?;

        node_from_row(&row)
    }

    async fn insert_in_txless(
        &self,
        labels: Vec<String>,
        properties: Properties,
    ) -> Result<Node, sqlx::Error> {
        let row = sqlx::query(
            r#"
            INSERT INTO nodes (labels, properties)
            VALUES ($1, $2)
            RETURNING id, labels, properties
            "#,
        )
        .bind(labels)
        .bind(Json(properties))
        .fetch_one(&self.pool)
        .await?;

        node_from_row(&row)
    }

    pub async fn get(&self, id: i64) -> Result<Option<Node>, sqlx::Error> {
        let row = sqlx::query(
            r#"
            SELECT id, labels, properties
            FROM nodes
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(|row| node_from_row(&row)).transpose()
    }

    pub async fn list(&self) -> Result<Vec<Node>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT id, labels, properties
            FROM nodes
            ORDER BY id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(node_from_row).collect()
    }

    pub async fn delete(&self, id: i64) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            r#"
            DELETE FROM nodes
            WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn find_by_label_property(
        &self,
        label: &str,
        property_key: &str,
        property_value: Value,
    ) -> Result<Vec<Node>, sqlx::Error> {
        let property_value = value_to_json(property_value);
        let rows = sqlx::query(
            r#"
            SELECT id, labels, properties
            FROM nodes
            WHERE labels @> ARRAY[$1::text]
              AND properties @> jsonb_build_object($2::text, $3::jsonb)
            ORDER BY id
            "#,
        )
        .bind(label)
        .bind(property_key)
        .bind(Json(property_value))
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(node_from_row).collect()
    }
}

#[derive(Debug, Clone)]
pub struct RelRepo {
    pool: PgPool,
}

impl RelRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn insert(
        &self,
        rel_type: String,
        start_id: i64,
        end_id: i64,
        properties: Properties,
    ) -> Result<Relationship, sqlx::Error> {
        self.insert_in_txless(rel_type, start_id, end_id, properties)
            .await
    }

    pub async fn insert_in_tx(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        rel_type: String,
        start_id: i64,
        end_id: i64,
        properties: Properties,
    ) -> Result<Relationship, sqlx::Error> {
        let row = sqlx::query(
            r#"
            INSERT INTO relationships (type, start_id, end_id, properties)
            VALUES ($1, $2, $3, $4)
            RETURNING id, type, start_id, end_id, properties
            "#,
        )
        .bind(rel_type)
        .bind(start_id)
        .bind(end_id)
        .bind(Json(properties))
        .fetch_one(&mut **transaction)
        .await?;

        relationship_from_row(&row)
    }

    async fn insert_in_txless(
        &self,
        rel_type: String,
        start_id: i64,
        end_id: i64,
        properties: Properties,
    ) -> Result<Relationship, sqlx::Error> {
        let row = sqlx::query(
            r#"
            INSERT INTO relationships (type, start_id, end_id, properties)
            VALUES ($1, $2, $3, $4)
            RETURNING id, type, start_id, end_id, properties
            "#,
        )
        .bind(rel_type)
        .bind(start_id)
        .bind(end_id)
        .bind(Json(properties))
        .fetch_one(&self.pool)
        .await?;

        relationship_from_row(&row)
    }

    pub async fn get(&self, id: i64) -> Result<Option<Relationship>, sqlx::Error> {
        let row = sqlx::query(
            r#"
            SELECT id, type, start_id, end_id, properties
            FROM relationships
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(|row| relationship_from_row(&row)).transpose()
    }

    pub async fn list_by_type(&self, rel_type: &str) -> Result<Vec<Relationship>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT id, type, start_id, end_id, properties
            FROM relationships
            WHERE type = $1
            ORDER BY id
            "#,
        )
        .bind(rel_type)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(relationship_from_row).collect()
    }

    pub async fn list(&self) -> Result<Vec<Relationship>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT id, type, start_id, end_id, properties
            FROM relationships
            ORDER BY id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(relationship_from_row).collect()
    }

    pub async fn delete(&self, id: i64) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            r#"
            DELETE FROM relationships
            WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn delete_by_node(&self, node_id: i64) -> Result<u64, sqlx::Error> {
        let result = sqlx::query(
            r#"
            DELETE FROM relationships
            WHERE start_id = $1 OR end_id = $1
            "#,
        )
        .bind(node_id)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }
}

fn node_from_row(row: &sqlx::postgres::PgRow) -> Result<Node, sqlx::Error> {
    Ok(Node {
        id: row.try_get("id")?,
        labels: row.try_get("labels")?,
        properties: row.try_get::<Json<Properties>, _>("properties")?.0,
    })
}

fn relationship_from_row(row: &sqlx::postgres::PgRow) -> Result<Relationship, sqlx::Error> {
    Ok(Relationship {
        id: row.try_get("id")?,
        r#type: row.try_get("type")?,
        start_id: row.try_get("start_id")?,
        end_id: row.try_get("end_id")?,
        properties: row.try_get::<Json<Properties>, _>("properties")?.0,
    })
}

fn value_to_json(value: Value) -> JsonValue {
    match value {
        Value::String(value) => JsonValue::String(value),
        Value::Number(value) => JsonValue::Number(value),
        Value::Bool(value) => JsonValue::Bool(value),
        Value::Null => JsonValue::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::{create_pool, NodeRepo, RelRepo};
    use crate::domain::{Properties, Value};
    use serde_json::{json, Number, Value as JsonValue};
    use std::sync::OnceLock;
    use tokio::sync::Mutex;

    static DB_TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

    #[tokio::test]
    async fn node_repo_supports_insert_get_list_delete_and_find() {
        let _guard = DB_TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().await;
        let Some(pool) = test_pool().await else {
            return;
        };
        ensure_schema(&pool).await;
        reset_tables(&pool).await;

        let repo = NodeRepo::new(pool.clone());

        let mut alpha_properties = Properties::new();
        alpha_properties.insert("name".to_string(), JsonValue::String("alpha".to_string()));
        alpha_properties.insert("priority".to_string(), JsonValue::Number(Number::from(7)));
        let inserted = repo
            .insert(
                vec!["Service".to_string(), "Critical".to_string()],
                alpha_properties.clone(),
            )
            .await
            .expect("node insert should succeed");

        let fetched = repo
            .get(inserted.id)
            .await
            .expect("node get should succeed")
            .expect("node should exist");
        assert_eq!(fetched, inserted);

        let mut beta_properties = Properties::new();
        beta_properties.insert("name".to_string(), JsonValue::String("beta".to_string()));
        repo.insert(vec!["Service".to_string()], beta_properties)
            .await
            .expect("second node insert should succeed");

        let listed = repo.list().await.expect("node list should succeed");
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0], inserted);

        let found = repo
            .find_by_label_property("Service", "name", Value::String("alpha".to_string()))
            .await
            .expect("find by label and property should succeed");
        assert_eq!(found, vec![inserted.clone()]);

        let not_found = repo
            .find_by_label_property("Critical", "priority", Value::Number(Number::from(8)))
            .await
            .expect("non-matching search should still succeed");
        assert!(not_found.is_empty());

        let deleted = repo
            .delete(inserted.id)
            .await
            .expect("node delete should succeed");
        assert!(deleted);
        assert!(repo
            .get(inserted.id)
            .await
            .expect("node get after delete should succeed")
            .is_none());
    }

    #[tokio::test]
    async fn relationship_repo_supports_insert_get_list_delete_and_delete_by_node() {
        let _guard = DB_TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().await;
        let Some(pool) = test_pool().await else {
            return;
        };
        ensure_schema(&pool).await;
        reset_tables(&pool).await;

        let node_repo = NodeRepo::new(pool.clone());
        let rel_repo = RelRepo::new(pool.clone());

        let start = node_repo
            .insert(vec!["Service".to_string()], Properties::new())
            .await
            .expect("start node insert should succeed");
        let end = node_repo
            .insert(vec!["Database".to_string()], Properties::new())
            .await
            .expect("end node insert should succeed");
        let other = node_repo
            .insert(vec!["Queue".to_string()], Properties::new())
            .await
            .expect("other node insert should succeed");

        let mut rel_properties = Properties::new();
        rel_properties.insert("weight".to_string(), json!(3));
        let inserted = rel_repo
            .insert(
                "DEPENDS_ON".to_string(),
                start.id,
                end.id,
                rel_properties.clone(),
            )
            .await
            .expect("relationship insert should succeed");
        rel_repo
            .insert(
                "PUBLISHES_TO".to_string(),
                other.id,
                end.id,
                Properties::new(),
            )
            .await
            .expect("second relationship insert should succeed");

        let fetched = rel_repo
            .get(inserted.id)
            .await
            .expect("relationship get should succeed")
            .expect("relationship should exist");
        assert_eq!(fetched, inserted);

        let listed = rel_repo
            .list()
            .await
            .expect("relationship list should succeed");
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0], inserted);

        let deleted = rel_repo
            .delete(inserted.id)
            .await
            .expect("relationship delete should succeed");
        assert!(deleted);
        assert!(rel_repo
            .get(inserted.id)
            .await
            .expect("relationship get after delete should succeed")
            .is_none());

        let deleted_by_node = rel_repo
            .delete_by_node(end.id)
            .await
            .expect("relationship delete by node should succeed");
        assert_eq!(deleted_by_node, 1);

        let after_delete = rel_repo
            .list()
            .await
            .expect("relationship list after delete should succeed");
        assert!(after_delete.is_empty());
    }

    async fn test_pool() -> Option<sqlx::PgPool> {
        let database_url = std::env::var("ZEROCLAW_TEST_DATABASE_URL")
            .unwrap_or_else(|_| include_str!("../../.database_url").trim().to_string());

        match create_pool(&database_url).await {
            Ok(pool) => Some(pool),
            Err(error) => {
                eprintln!("skipping database test because the configured database is unavailable: {error}");
                None
            }
        }
    }

    async fn ensure_schema(pool: &sqlx::PgPool) {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS nodes (
                id BIGSERIAL PRIMARY KEY,
                labels TEXT[] NOT NULL DEFAULT '{}'::TEXT[],
                properties JSONB NOT NULL DEFAULT '{}'::JSONB
            );

            CREATE TABLE IF NOT EXISTS relationships (
                id BIGSERIAL PRIMARY KEY,
                type TEXT NOT NULL,
                start_id BIGINT NOT NULL,
                end_id BIGINT NOT NULL,
                properties JSONB NOT NULL DEFAULT '{}'::JSONB
            );

            CREATE INDEX IF NOT EXISTS nodes_labels_gin_idx ON nodes USING GIN (labels);
            CREATE INDEX IF NOT EXISTS relationships_type_idx ON relationships (type);
            CREATE INDEX IF NOT EXISTS relationships_start_id_idx ON relationships (start_id);
            CREATE INDEX IF NOT EXISTS relationships_end_id_idx ON relationships (end_id);
            "#,
        )
        .execute(pool)
        .await
        .expect("schema setup should succeed");
    }

    async fn reset_tables(pool: &sqlx::PgPool) {
        sqlx::query(
            r#"
            TRUNCATE TABLE relationships, nodes RESTART IDENTITY
            "#,
        )
        .execute(pool)
        .await
        .expect("table reset should succeed");
    }
}
