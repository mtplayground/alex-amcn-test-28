use std::collections::{BTreeSet, HashMap};
use std::fmt;

use sqlx::PgPool;

use crate::{
    domain::{Node, NodeId, RelId, Relationship},
    graph::GraphIndex,
    parser::{DeleteClause, Expression},
};

#[derive(Debug, Clone, PartialEq)]
pub enum BoundValue {
    Node(Node),
    Relationship(Relationship),
}

pub type Binding = HashMap<String, BoundValue>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteSummary {
    pub deleted_nodes: Vec<NodeId>,
    pub deleted_relationships: Vec<RelId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutorError {
    pub message: String,
}

impl fmt::Display for ExecutorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ExecutorError {}

impl From<sqlx::Error> for ExecutorError {
    fn from(error: sqlx::Error) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}

pub async fn execute_delete(
    pool: &PgPool,
    graph_index: &mut GraphIndex,
    bindings: &[Binding],
    clause: &DeleteClause,
) -> Result<DeleteSummary, ExecutorError> {
    let deletion_plan = build_delete_plan(graph_index, bindings, clause)?;
    let mut transaction = pool.begin().await?;

    for rel_id in &deletion_plan.deleted_relationships {
        let result = sqlx::query(
            r#"
            DELETE FROM relationships
            WHERE id = $1
            "#,
        )
        .bind(rel_id)
        .execute(&mut *transaction)
        .await?;

        if result.rows_affected() == 0 {
            return Err(ExecutorError {
                message: format!("relationship {rel_id} was not found"),
            });
        }
    }

    for node_id in &deletion_plan.deleted_nodes {
        let result = sqlx::query(
            r#"
            DELETE FROM nodes
            WHERE id = $1
            "#,
        )
        .bind(node_id)
        .execute(&mut *transaction)
        .await?;

        if result.rows_affected() == 0 {
            return Err(ExecutorError {
                message: format!("node {node_id} was not found"),
            });
        }
    }

    transaction.commit().await?;

    for rel_id in &deletion_plan.deleted_relationships {
        graph_index.remove_rel(*rel_id);
    }

    for node_id in &deletion_plan.deleted_nodes {
        graph_index.remove_node(*node_id);
    }

    Ok(deletion_plan)
}

fn build_delete_plan(
    graph_index: &GraphIndex,
    bindings: &[Binding],
    clause: &DeleteClause,
) -> Result<DeleteSummary, ExecutorError> {
    let mut node_ids = BTreeSet::new();
    let mut relationship_ids = BTreeSet::new();

    for expression in &clause.expressions {
        let identifier = match expression {
            Expression::Identifier(identifier) => identifier,
            _ => {
                return Err(ExecutorError {
                    message: "DELETE expressions must be identifiers".to_string(),
                });
            }
        };

        for binding in bindings {
            let Some(value) = binding.get(identifier) else {
                return Err(ExecutorError {
                    message: format!("unbound identifier '{identifier}' in DELETE clause"),
                });
            };

            match value {
                BoundValue::Node(node) => {
                    let incident_relationships = graph_index.incident_rel_ids(node.id);
                    if !clause.detach && !incident_relationships.is_empty() {
                        return Err(ExecutorError {
                            message: format!(
                                "cannot DELETE node '{}' with existing relationships; use DETACH DELETE",
                                identifier
                            ),
                        });
                    }

                    node_ids.insert(node.id);
                    relationship_ids.extend(incident_relationships);
                }
                BoundValue::Relationship(relationship) => {
                    relationship_ids.insert(relationship.id);
                }
            }
        }
    }

    Ok(DeleteSummary {
        deleted_nodes: node_ids.into_iter().collect(),
        deleted_relationships: relationship_ids.into_iter().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::{execute_delete, Binding, BoundValue};
    use crate::{
        db::{create_pool, NodeRepo, RelRepo},
        domain::Properties,
        graph::GraphIndex,
        parser::{parse, Clause},
    };
    use std::sync::OnceLock;
    use tokio::sync::Mutex;

    static DB_TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

    #[tokio::test]
    async fn detach_delete_removes_nodes_and_incident_relationships_from_db_and_index() {
        let _guard = DB_TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().await;
        let Some(pool) = test_pool().await else {
            return;
        };
        ensure_schema(&pool).await;
        reset_tables(&pool).await;

        let node_repo = NodeRepo::new(pool.clone());
        let rel_repo = RelRepo::new(pool.clone());

        let alpha = node_repo
            .insert(vec!["Service".to_string()], Properties::new())
            .await
            .expect("alpha insert should succeed");
        let beta = node_repo
            .insert(vec!["Database".to_string()], Properties::new())
            .await
            .expect("beta insert should succeed");
        let gamma = node_repo
            .insert(vec!["Cache".to_string()], Properties::new())
            .await
            .expect("gamma insert should succeed");
        let rel_one = rel_repo
            .insert(
                "DEPENDS_ON".to_string(),
                alpha.id,
                beta.id,
                Properties::new(),
            )
            .await
            .expect("first relationship insert should succeed");
        let rel_two = rel_repo
            .insert(
                "FEEDS".to_string(),
                gamma.id,
                alpha.id,
                Properties::new(),
            )
            .await
            .expect("second relationship insert should succeed");

        let mut graph_index = GraphIndex::load_from_db(&pool)
            .await
            .expect("graph index should load");
        let parsed = parse("DETACH DELETE n").expect("query should parse");
        let clause = delete_clause(&parsed);
        let bindings = vec![binding([("n", BoundValue::Node(alpha.clone()))])];

        let summary = execute_delete(&pool, &mut graph_index, &bindings, &clause)
            .await
            .expect("detach delete should succeed");

        assert_eq!(summary.deleted_nodes, vec![alpha.id]);
        assert_eq!(summary.deleted_relationships, vec![rel_one.id, rel_two.id]);
        assert!(node_repo.get(alpha.id).await.expect("node lookup should succeed").is_none());
        assert!(rel_repo.get(rel_one.id).await.expect("rel lookup should succeed").is_none());
        assert!(rel_repo.get(rel_two.id).await.expect("rel lookup should succeed").is_none());
        assert!(!graph_index.contains_node(alpha.id));
        assert!(!graph_index.contains_rel(rel_one.id));
        assert!(!graph_index.contains_rel(rel_two.id));
        assert!(graph_index.contains_node(beta.id));
        assert!(graph_index.contains_node(gamma.id));
    }

    #[tokio::test]
    async fn delete_relationship_only_removes_relationship_from_db_and_index() {
        let _guard = DB_TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().await;
        let Some(pool) = test_pool().await else {
            return;
        };
        ensure_schema(&pool).await;
        reset_tables(&pool).await;

        let node_repo = NodeRepo::new(pool.clone());
        let rel_repo = RelRepo::new(pool.clone());

        let alpha = node_repo
            .insert(vec!["Service".to_string()], Properties::new())
            .await
            .expect("alpha insert should succeed");
        let beta = node_repo
            .insert(vec!["Database".to_string()], Properties::new())
            .await
            .expect("beta insert should succeed");
        let relationship = rel_repo
            .insert(
                "DEPENDS_ON".to_string(),
                alpha.id,
                beta.id,
                Properties::new(),
            )
            .await
            .expect("relationship insert should succeed");

        let mut graph_index = GraphIndex::load_from_db(&pool)
            .await
            .expect("graph index should load");
        let parsed = parse("DELETE r").expect("query should parse");
        let clause = delete_clause(&parsed);
        let bindings = vec![binding([("r", BoundValue::Relationship(relationship.clone()))])];

        let summary = execute_delete(&pool, &mut graph_index, &bindings, &clause)
            .await
            .expect("relationship delete should succeed");

        assert!(summary.deleted_nodes.is_empty());
        assert_eq!(summary.deleted_relationships, vec![relationship.id]);
        assert!(rel_repo
            .get(relationship.id)
            .await
            .expect("relationship lookup should succeed")
            .is_none());
        assert!(graph_index.contains_node(alpha.id));
        assert!(graph_index.contains_node(beta.id));
        assert!(!graph_index.contains_rel(relationship.id));
    }

    #[tokio::test]
    async fn delete_node_without_detach_rejects_incident_relationships() {
        let _guard = DB_TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().await;
        let Some(pool) = test_pool().await else {
            return;
        };
        ensure_schema(&pool).await;
        reset_tables(&pool).await;

        let node_repo = NodeRepo::new(pool.clone());
        let rel_repo = RelRepo::new(pool.clone());

        let alpha = node_repo
            .insert(vec!["Service".to_string()], Properties::new())
            .await
            .expect("alpha insert should succeed");
        let beta = node_repo
            .insert(vec!["Database".to_string()], Properties::new())
            .await
            .expect("beta insert should succeed");
        let relationship = rel_repo
            .insert(
                "DEPENDS_ON".to_string(),
                alpha.id,
                beta.id,
                Properties::new(),
            )
            .await
            .expect("relationship insert should succeed");

        let mut graph_index = GraphIndex::load_from_db(&pool)
            .await
            .expect("graph index should load");
        let parsed = parse("DELETE n").expect("query should parse");
        let clause = delete_clause(&parsed);
        let bindings = vec![binding([("n", BoundValue::Node(alpha.clone()))])];

        let error = execute_delete(&pool, &mut graph_index, &bindings, &clause)
            .await
            .expect_err("delete should fail");

        assert_eq!(
            error.message,
            "cannot DELETE node 'n' with existing relationships; use DETACH DELETE"
        );
        assert!(node_repo
            .get(alpha.id)
            .await
            .expect("node lookup should succeed")
            .is_some());
        assert!(rel_repo
            .get(relationship.id)
            .await
            .expect("relationship lookup should succeed")
            .is_some());
        assert!(graph_index.contains_node(alpha.id));
        assert!(graph_index.contains_rel(relationship.id));
    }

    fn delete_clause(parsed: &crate::parser::Query) -> crate::parser::DeleteClause {
        match &parsed.clauses[0] {
            Clause::Delete(clause) => clause.clone(),
            _ => panic!("expected delete clause"),
        }
    }

    fn binding<const N: usize>(entries: [(&str, BoundValue); N]) -> Binding {
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect()
    }

    async fn test_pool() -> Option<sqlx::PgPool> {
        let database_url = std::env::var("ZEROCLAW_TEST_DATABASE_URL")
            .unwrap_or_else(|_| include_str!("../../.database_url").trim().to_string());

        match create_pool(&database_url).await {
            Ok(pool) => Some(pool),
            Err(error) => {
                eprintln!(
                    "skipping database test because the configured database is unavailable: {error}"
                );
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
