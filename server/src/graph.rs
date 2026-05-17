use std::collections::HashMap;

use petgraph::graph::{EdgeIndex, Graph, NodeIndex};
use petgraph::Direction;
use sqlx::PgPool;

use crate::{
    db::{NodeRepo, RelRepo},
    domain::{NodeId, RelId},
};

#[derive(Debug, Default)]
pub struct GraphIndex {
    graph: Graph<NodeId, RelId>,
    node_indices: HashMap<NodeId, NodeIndex>,
    edge_indices: HashMap<RelId, EdgeIndex>,
}

impl GraphIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn load_from_db(pool: &PgPool) -> Result<Self, sqlx::Error> {
        let node_repo = NodeRepo::new(pool.clone());
        let rel_repo = RelRepo::new(pool.clone());
        let nodes = node_repo.list().await?;
        let relationships = rel_repo.list().await?;

        let mut index = Self::new();

        for node in nodes {
            index.add_node(node.id);
        }

        for relationship in relationships {
            index.add_rel(relationship.id, relationship.start_id, relationship.end_id);
        }

        Ok(index)
    }

    pub fn add_node(&mut self, node_id: NodeId) -> NodeIndex {
        if let Some(index) = self.node_indices.get(&node_id).copied() {
            return index;
        }

        let index = self.graph.add_node(node_id);
        self.node_indices.insert(node_id, index);
        index
    }

    pub fn add_rel(&mut self, rel_id: RelId, start_id: NodeId, end_id: NodeId) -> EdgeIndex {
        if let Some(index) = self.edge_indices.get(&rel_id).copied() {
            return index;
        }

        let start_index = self.add_node(start_id);
        let end_index = self.add_node(end_id);
        let edge_index = self.graph.add_edge(start_index, end_index, rel_id);
        self.edge_indices.insert(rel_id, edge_index);
        edge_index
    }

    pub fn remove_node(&mut self, node_id: NodeId) -> bool {
        let Some(node_index) = self.node_indices.remove(&node_id) else {
            return false;
        };

        let outgoing_edge_ids = self
            .graph
            .edges_directed(node_index, Direction::Outgoing)
            .map(|edge| *edge.weight())
            .collect::<Vec<_>>();
        let incoming_edge_ids = self
            .graph
            .edges_directed(node_index, Direction::Incoming)
            .map(|edge| *edge.weight())
            .collect::<Vec<_>>();

        for rel_id in outgoing_edge_ids.into_iter().chain(incoming_edge_ids) {
            self.edge_indices.remove(&rel_id);
        }

        self.graph.remove_node(node_index);
        self.rebuild_mappings();
        true
    }

    pub fn remove_rel(&mut self, rel_id: RelId) -> bool {
        let Some(edge_index) = self.edge_indices.remove(&rel_id) else {
            return false;
        };

        let removed = self.graph.remove_edge(edge_index).is_some();
        if removed {
            self.rebuild_mappings();
        }
        removed
    }

    pub fn node_count(&self) -> usize {
        self.node_indices.len()
    }

    pub fn relationship_count(&self) -> usize {
        self.edge_indices.len()
    }

    pub fn contains_node(&self, node_id: NodeId) -> bool {
        self.node_indices.contains_key(&node_id)
    }

    pub fn contains_rel(&self, rel_id: RelId) -> bool {
        self.edge_indices.contains_key(&rel_id)
    }

    fn rebuild_mappings(&mut self) {
        self.node_indices = self
            .graph
            .node_indices()
            .map(|index| (self.graph[index], index))
            .collect();
        self.edge_indices = self
            .graph
            .edge_indices()
            .map(|index| (self.graph[index], index))
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::GraphIndex;
    use crate::{
        db::{create_pool, NodeRepo, RelRepo},
        domain::Properties,
    };
    use std::sync::OnceLock;
    use tokio::sync::Mutex;

    static DB_TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn add_and_remove_keep_index_mappings_in_sync() {
        let mut index = GraphIndex::new();

        index.add_node(1);
        index.add_node(2);
        index.add_node(3);
        index.add_rel(11, 1, 2);
        index.add_rel(12, 2, 3);

        assert_eq!(index.node_count(), 3);
        assert_eq!(index.relationship_count(), 2);
        assert!(index.contains_node(1));
        assert!(index.contains_rel(11));

        assert!(index.remove_rel(11));
        assert_eq!(index.relationship_count(), 1);
        assert!(!index.contains_rel(11));
        assert!(index.contains_rel(12));

        assert!(index.remove_node(1));
        assert_eq!(index.node_count(), 2);
        assert_eq!(index.relationship_count(), 1);
        assert!(!index.contains_node(1));
        assert!(index.contains_rel(12));

        assert!(index.remove_node(2));
        assert_eq!(index.node_count(), 1);
        assert_eq!(index.relationship_count(), 0);
        assert!(!index.contains_rel(12));
        assert!(index.contains_node(3));
    }

    #[tokio::test]
    async fn load_from_db_builds_graph_from_existing_rows() {
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
        let relationship = rel_repo
            .insert(
                "DEPENDS_ON".to_string(),
                start.id,
                end.id,
                Properties::new(),
            )
            .await
            .expect("relationship insert should succeed");

        let index = GraphIndex::load_from_db(&pool)
            .await
            .expect("graph load should succeed");

        assert_eq!(index.node_count(), 2);
        assert_eq!(index.relationship_count(), 1);
        assert!(index.contains_node(start.id));
        assert!(index.contains_node(end.id));
        assert!(index.contains_rel(relationship.id));
    }

    #[tokio::test]
    async fn graph_index_stays_in_sync_after_each_repo_mutation() {
        let _guard = DB_TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().await;
        let Some(pool) = test_pool().await else {
            return;
        };
        ensure_schema(&pool).await;
        reset_tables(&pool).await;

        let node_repo = NodeRepo::new(pool.clone());
        let rel_repo = RelRepo::new(pool.clone());
        let mut index = GraphIndex::load_from_db(&pool)
            .await
            .expect("graph load should succeed");

        assert_eq!(index.node_count(), 0);
        assert_eq!(index.relationship_count(), 0);

        let alpha = node_repo
            .insert(vec!["Service".to_string()], Properties::new())
            .await
            .expect("alpha node insert should succeed");
        index.add_node(alpha.id);
        assert_eq!(index.node_count(), 1);
        assert!(index.contains_node(alpha.id));

        let beta = node_repo
            .insert(vec!["Database".to_string()], Properties::new())
            .await
            .expect("beta node insert should succeed");
        index.add_node(beta.id);
        assert_eq!(index.node_count(), 2);
        assert!(index.contains_node(beta.id));

        let relationship = rel_repo
            .insert(
                "DEPENDS_ON".to_string(),
                alpha.id,
                beta.id,
                Properties::new(),
            )
            .await
            .expect("relationship insert should succeed");
        index.add_rel(relationship.id, relationship.start_id, relationship.end_id);
        assert_eq!(index.relationship_count(), 1);
        assert!(index.contains_rel(relationship.id));

        let rel_deleted = rel_repo
            .delete(relationship.id)
            .await
            .expect("relationship delete should succeed");
        assert!(rel_deleted);
        assert!(index.remove_rel(relationship.id));
        assert_eq!(index.relationship_count(), 0);
        assert!(!index.contains_rel(relationship.id));

        let beta_deleted = node_repo
            .delete(beta.id)
            .await
            .expect("beta node delete should succeed");
        assert!(beta_deleted);
        assert!(index.remove_node(beta.id));
        assert_eq!(index.node_count(), 1);
        assert!(!index.contains_node(beta.id));

        let alpha_deleted = node_repo
            .delete(alpha.id)
            .await
            .expect("alpha node delete should succeed");
        assert!(alpha_deleted);
        assert!(index.remove_node(alpha.id));
        assert_eq!(index.node_count(), 0);
        assert!(!index.contains_node(alpha.id));
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
