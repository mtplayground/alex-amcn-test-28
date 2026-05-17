use std::collections::{BTreeSet, HashMap};
use std::fmt;

use serde_json::Value as JsonValue;
use sqlx::{PgPool, Postgres, Transaction};

use crate::{
    db::{NodeRepo, RelRepo},
    domain::{Node, NodeId, Properties, RelId, Relationship},
    graph::GraphIndex,
    parser::{
        BinaryOperator, Clause, CreateClause, DeleteClause, Expression, Literal, NodePattern,
        Query, RelationshipPattern,
    },
};

#[derive(Debug, Clone, PartialEq)]
pub enum BoundValue {
    Node(Node),
    Relationship(Relationship),
}

pub type Binding = HashMap<String, BoundValue>;

#[derive(Debug, Clone, PartialEq)]
pub struct CreateSummary {
    pub bindings: Binding,
    pub created_nodes: Vec<Node>,
    pub created_relationships: Vec<Relationship>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchSummary {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<JsonValue>>,
    pub bindings: Vec<Binding>,
}

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

pub async fn execute_create(
    pool: &PgPool,
    graph_index: &mut GraphIndex,
    clause: &CreateClause,
) -> Result<CreateSummary, ExecutorError> {
    let node_repo = NodeRepo::new(pool.clone());
    let rel_repo = RelRepo::new(pool.clone());
    let mut transaction = pool.begin().await?;
    let mut bindings = Binding::new();
    let mut created_nodes = Vec::new();
    let mut created_relationships = Vec::new();

    for pattern in &clause.patterns {
        let mut current_node = materialize_node_pattern(
            &node_repo,
            &mut transaction,
            &mut bindings,
            &mut created_nodes,
            &pattern.start,
        )
        .await?;

        for chain in &pattern.chains {
            let next_node = materialize_node_pattern(
                &node_repo,
                &mut transaction,
                &mut bindings,
                &mut created_nodes,
                &chain.node,
            )
            .await?;
            let relationship = materialize_relationship_pattern(
                &rel_repo,
                &mut transaction,
                &mut bindings,
                &mut created_relationships,
                &chain.relationship,
                current_node.id,
                next_node.id,
            )
            .await?;

            if let Some(variable) = &chain.relationship.variable {
                bindings.insert(variable.clone(), BoundValue::Relationship(relationship));
            }

            current_node = next_node;
        }
    }

    transaction.commit().await?;

    for node in &created_nodes {
        graph_index.add_node(node.id);
    }

    for relationship in &created_relationships {
        graph_index.add_rel(relationship.id, relationship.start_id, relationship.end_id);
    }

    Ok(CreateSummary {
        bindings,
        created_nodes,
        created_relationships,
    })
}

pub async fn execute_match_query(
    pool: &PgPool,
    graph_index: &GraphIndex,
    query: &Query,
) -> Result<MatchSummary, ExecutorError> {
    let node_repo = NodeRepo::new(pool.clone());
    let rel_repo = RelRepo::new(pool.clone());
    let nodes = node_repo.list().await?;
    let relationships = rel_repo.list().await?;
    let node_map = nodes
        .into_iter()
        .map(|node| (node.id, node))
        .collect::<HashMap<_, _>>();
    let relationship_map = relationships
        .into_iter()
        .map(|relationship| (relationship.id, relationship))
        .collect::<HashMap<_, _>>();

    let match_clause = extract_single_clause(query, |clause| match clause {
        Clause::Match(clause) => Some(clause),
        _ => None,
    })?;
    let where_clause = extract_optional_clause(query, |clause| match clause {
        Clause::Where(clause) => Some(clause),
        _ => None,
    })?;
    let return_clause = extract_single_clause(query, |clause| match clause {
        Clause::Return(clause) => Some(clause),
        _ => None,
    })?;
    let limit_clause = extract_optional_clause(query, |clause| match clause {
        Clause::Limit(clause) => Some(clause),
        _ => None,
    })?;

    let mut bindings = vec![Binding::new()];
    for pattern in &match_clause.patterns {
        let mut next_bindings = Vec::new();
        for binding in &bindings {
            extend_pattern_matches(
                pattern,
                binding,
                graph_index,
                &node_map,
                &relationship_map,
                &mut next_bindings,
            )?;
        }
        bindings = next_bindings;
    }

    if let Some(where_clause) = where_clause {
        bindings = bindings
            .into_iter()
            .filter(|binding| evaluate_predicate(&where_clause.expression, binding).unwrap_or(false))
            .collect();
    }

    if let Some(limit_clause) = limit_clause {
        bindings.truncate(limit_clause.count);
    }

    let columns = return_clause
        .items
        .iter()
        .map(|projection| projection_name(&projection.expression))
        .collect::<Vec<_>>();
    let rows = bindings
        .iter()
        .map(|binding| {
            return_clause
                .items
                .iter()
                .map(|projection| project_value(&projection.expression, binding))
                .collect::<Result<Vec<_>, ExecutorError>>()
        })
        .collect::<Result<Vec<_>, ExecutorError>>()?;

    Ok(MatchSummary {
        columns,
        rows,
        bindings,
    })
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

fn extend_pattern_matches(
    pattern: &crate::parser::Pattern,
    binding: &Binding,
    graph_index: &GraphIndex,
    nodes: &HashMap<NodeId, Node>,
    relationships: &HashMap<RelId, Relationship>,
    results: &mut Vec<Binding>,
) -> Result<(), ExecutorError> {
    for start_node in candidate_nodes(&pattern.start, binding, nodes)? {
        let Some(bound_start) = bind_node_pattern(binding, &pattern.start, start_node)? else {
            continue;
        };

        walk_pattern_chain(
            &pattern.chains,
            0,
            bound_start,
            start_node,
            graph_index,
            nodes,
            relationships,
            results,
        )?;
    }

    Ok(())
}

fn walk_pattern_chain(
    chains: &[crate::parser::PatternChain],
    index: usize,
    binding: Binding,
    current_node: &Node,
    graph_index: &GraphIndex,
    nodes: &HashMap<NodeId, Node>,
    relationships: &HashMap<RelId, Relationship>,
    results: &mut Vec<Binding>,
) -> Result<(), ExecutorError> {
    if index == chains.len() {
        results.push(binding);
        return Ok(());
    }

    let chain = &chains[index];
    for rel_id in graph_index.outgoing_rel_ids(current_node.id) {
        let Some(relationship) = relationships.get(&rel_id) else {
            continue;
        };
        if relationship.start_id != current_node.id {
            continue;
        }

        let Some(bound_rel) = bind_relationship_pattern(&binding, &chain.relationship, relationship)?
        else {
            continue;
        };
        let Some(next_node) = nodes.get(&relationship.end_id) else {
            continue;
        };
        let Some(bound_node) = bind_node_pattern(&bound_rel, &chain.node, next_node)? else {
            continue;
        };

        walk_pattern_chain(
            chains,
            index + 1,
            bound_node,
            next_node,
            graph_index,
            nodes,
            relationships,
            results,
        )?;
    }

    Ok(())
}

fn candidate_nodes<'a>(
    pattern: &NodePattern,
    binding: &'a Binding,
    nodes: &'a HashMap<NodeId, Node>,
) -> Result<Vec<&'a Node>, ExecutorError> {
    if let Some(variable) = &pattern.variable {
        if let Some(bound_value) = binding.get(variable) {
            return match bound_value {
                BoundValue::Node(node) => Ok(nodes.get(&node.id).into_iter().collect()),
                BoundValue::Relationship(_) => Err(ExecutorError {
                    message: format!("variable '{}' is already bound to a relationship", variable),
                }),
            };
        }
    }

    let mut candidates = nodes.values().collect::<Vec<_>>();
    candidates.sort_by_key(|node| node.id);
    Ok(candidates)
}

fn bind_node_pattern(
    binding: &Binding,
    pattern: &NodePattern,
    candidate: &Node,
) -> Result<Option<Binding>, ExecutorError> {
    if !node_matches_pattern(candidate, pattern) {
        return Ok(None);
    }

    let mut next = binding.clone();
    if let Some(variable) = &pattern.variable {
        match binding.get(variable) {
            Some(BoundValue::Node(node)) if node.id == candidate.id => {}
            Some(BoundValue::Node(_)) => return Ok(None),
            Some(BoundValue::Relationship(_)) => {
                return Err(ExecutorError {
                    message: format!("variable '{}' is already bound to a relationship", variable),
                });
            }
            None => {
                next.insert(variable.clone(), BoundValue::Node(candidate.clone()));
            }
        }
    }

    Ok(Some(next))
}

fn bind_relationship_pattern(
    binding: &Binding,
    pattern: &RelationshipPattern,
    candidate: &Relationship,
) -> Result<Option<Binding>, ExecutorError> {
    if !relationship_matches_pattern(candidate, pattern) {
        return Ok(None);
    }

    let mut next = binding.clone();
    if let Some(variable) = &pattern.variable {
        match binding.get(variable) {
            Some(BoundValue::Relationship(relationship)) if relationship.id == candidate.id => {}
            Some(BoundValue::Relationship(_)) => return Ok(None),
            Some(BoundValue::Node(_)) => {
                return Err(ExecutorError {
                    message: format!("variable '{}' is already bound to a node", variable),
                });
            }
            None => {
                next.insert(variable.clone(), BoundValue::Relationship(candidate.clone()));
            }
        }
    }

    Ok(Some(next))
}

fn node_matches_pattern(node: &Node, pattern: &NodePattern) -> bool {
    pattern
        .labels
        .iter()
        .all(|label| node.labels.iter().any(|node_label| node_label == label))
        && properties_match(&node.properties, &pattern.properties)
}

fn relationship_matches_pattern(
    relationship: &Relationship,
    pattern: &RelationshipPattern,
) -> bool {
    pattern
        .rel_type
        .as_ref()
        .is_none_or(|rel_type| rel_type == &relationship.r#type)
        && properties_match(&relationship.properties, &pattern.properties)
}

fn properties_match(
    actual: &Properties,
    expected: &[crate::parser::Property],
) -> bool {
    let expected = properties_from_literals(expected);
    expected
        .iter()
        .all(|(key, value)| actual.get(key).is_some_and(|actual| actual == value))
}

fn evaluate_predicate(expression: &Expression, binding: &Binding) -> Result<bool, ExecutorError> {
    match expression {
        Expression::Binary {
            left,
            operator: BinaryOperator::And,
            right,
        } => Ok(evaluate_predicate(left, binding)? && evaluate_predicate(right, binding)?),
        Expression::Binary {
            left,
            operator: BinaryOperator::Or,
            right,
        } => Ok(evaluate_predicate(left, binding)? || evaluate_predicate(right, binding)?),
        Expression::Binary {
            left,
            operator,
            right,
        } => compare_values(
            &scalar_value(left, binding)?,
            operator,
            &scalar_value(right, binding)?,
        ),
        _ => match scalar_value(expression, binding)? {
            JsonValue::Bool(value) => Ok(value),
            _ => Err(ExecutorError {
                message: "WHERE expressions must evaluate to booleans".to_string(),
            }),
        },
    }
}

fn scalar_value(expression: &Expression, binding: &Binding) -> Result<JsonValue, ExecutorError> {
    match expression {
        Expression::Literal(literal) => Ok(literal_to_json(literal)),
        Expression::PropertyAccess {
            identifier,
            property,
        } => {
            let bound = binding.get(identifier).ok_or_else(|| ExecutorError {
                message: format!("unbound identifier '{}'", identifier),
            })?;
            Ok(match bound {
                BoundValue::Node(node) => node.properties.get(property).cloned().unwrap_or(JsonValue::Null),
                BoundValue::Relationship(relationship) => relationship
                    .properties
                    .get(property)
                    .cloned()
                    .unwrap_or(JsonValue::Null),
            })
        }
        Expression::Identifier(identifier) => {
            let bound = binding.get(identifier).ok_or_else(|| ExecutorError {
                message: format!("unbound identifier '{}'", identifier),
            })?;
            match bound {
                BoundValue::Node(_) | BoundValue::Relationship(_) => Err(ExecutorError {
                    message: format!(
                        "identifier '{}' does not resolve to a scalar value in this expression",
                        identifier
                    ),
                }),
            }
        }
        Expression::Binary { .. } => Ok(JsonValue::Bool(evaluate_predicate(expression, binding)?)),
    }
}

fn compare_values(
    left: &JsonValue,
    operator: &BinaryOperator,
    right: &JsonValue,
) -> Result<bool, ExecutorError> {
    use std::cmp::Ordering;

    let ordering = match (left, right) {
        (JsonValue::Number(left), JsonValue::Number(right)) => {
            left.as_f64()
                .zip(right.as_f64())
                .and_then(|(left, right)| left.partial_cmp(&right))
        }
        (JsonValue::String(left), JsonValue::String(right)) => Some(left.cmp(right)),
        (JsonValue::Bool(left), JsonValue::Bool(right)) => Some(left.cmp(right)),
        (JsonValue::Null, JsonValue::Null) => Some(Ordering::Equal),
        _ => None,
    };

    Ok(match operator {
        BinaryOperator::Equals => left == right,
        BinaryOperator::NotEquals => left != right,
        BinaryOperator::LessThan => ordering.is_some_and(|ordering| ordering == Ordering::Less),
        BinaryOperator::LessThanOrEqual => ordering.is_some_and(|ordering| {
            ordering == Ordering::Less || ordering == Ordering::Equal
        }),
        BinaryOperator::GreaterThan => {
            ordering.is_some_and(|ordering| ordering == Ordering::Greater)
        }
        BinaryOperator::GreaterThanOrEqual => ordering.is_some_and(|ordering| {
            ordering == Ordering::Greater || ordering == Ordering::Equal
        }),
        BinaryOperator::And | BinaryOperator::Or => {
            return Err(ExecutorError {
                message: "logical operators require boolean expressions".to_string(),
            });
        }
    })
}

fn project_value(expression: &Expression, binding: &Binding) -> Result<JsonValue, ExecutorError> {
    match expression {
        Expression::Identifier(identifier) => {
            let bound = binding.get(identifier).ok_or_else(|| ExecutorError {
                message: format!("unbound identifier '{}'", identifier),
            })?;
            match bound {
                BoundValue::Node(node) => serde_json::to_value(node).map_err(|error| ExecutorError {
                    message: error.to_string(),
                }),
                BoundValue::Relationship(relationship) => {
                    serde_json::to_value(relationship).map_err(|error| ExecutorError {
                        message: error.to_string(),
                    })
                }
            }
        }
        Expression::PropertyAccess { .. } | Expression::Literal(_) | Expression::Binary { .. } => {
            scalar_value(expression, binding)
        }
    }
}

fn projection_name(expression: &Expression) -> String {
    match expression {
        Expression::Identifier(identifier) => identifier.clone(),
        Expression::PropertyAccess {
            identifier,
            property,
        } => format!("{identifier}.{property}"),
        Expression::Literal(literal) => match literal {
            Literal::String(value) => value.clone(),
            Literal::Number(value) => value.clone(),
            Literal::Bool(value) => value.to_string(),
        },
        Expression::Binary { .. } => "expr".to_string(),
    }
}

fn literal_to_json(literal: &Literal) -> JsonValue {
    match literal {
        Literal::String(value) => JsonValue::String(value.clone()),
        Literal::Number(value) => json_number_from_lexer(value),
        Literal::Bool(value) => JsonValue::Bool(*value),
    }
}

fn extract_single_clause<'a, T>(
    query: &'a Query,
    selector: impl Fn(&'a Clause) -> Option<&'a T>,
) -> Result<&'a T, ExecutorError> {
    let mut matches = query.clauses.iter().filter_map(selector);
    let Some(first) = matches.next() else {
        return Err(ExecutorError {
            message: "query is missing a required clause".to_string(),
        });
    };
    if matches.next().is_some() {
        return Err(ExecutorError {
            message: "query contains multiple unsupported clauses of the same kind".to_string(),
        });
    }
    Ok(first)
}

fn extract_optional_clause<'a, T>(
    query: &'a Query,
    selector: impl Fn(&'a Clause) -> Option<&'a T>,
) -> Result<Option<&'a T>, ExecutorError> {
    let mut matches = query.clauses.iter().filter_map(selector);
    let first = matches.next();
    if matches.next().is_some() {
        return Err(ExecutorError {
            message: "query contains multiple unsupported clauses of the same kind".to_string(),
        });
    }
    Ok(first)
}

async fn materialize_node_pattern(
    node_repo: &NodeRepo,
    transaction: &mut Transaction<'_, Postgres>,
    bindings: &mut Binding,
    created_nodes: &mut Vec<Node>,
    pattern: &NodePattern,
) -> Result<Node, ExecutorError> {
    if let Some(variable) = &pattern.variable {
        if let Some(bound) = bindings.get(variable) {
            return match bound {
                BoundValue::Node(node) => {
                    if !pattern.labels.is_empty() || !pattern.properties.is_empty() {
                        Err(ExecutorError {
                            message: format!(
                                "cannot redefine previously-created node variable '{}'",
                                variable
                            ),
                        })
                    } else {
                        Ok(node.clone())
                    }
                }
                BoundValue::Relationship(_) => Err(ExecutorError {
                    message: format!("variable '{}' is already bound to a relationship", variable),
                }),
            };
        }
    }

    let node = node_repo
        .insert_in_tx(
            transaction,
            pattern.labels.clone(),
            properties_from_literals(&pattern.properties),
        )
        .await?;

    if let Some(variable) = &pattern.variable {
        bindings.insert(variable.clone(), BoundValue::Node(node.clone()));
    }

    created_nodes.push(node.clone());
    Ok(node)
}

async fn materialize_relationship_pattern(
    rel_repo: &RelRepo,
    transaction: &mut Transaction<'_, Postgres>,
    bindings: &mut Binding,
    created_relationships: &mut Vec<Relationship>,
    pattern: &RelationshipPattern,
    start_id: NodeId,
    end_id: NodeId,
) -> Result<Relationship, ExecutorError> {
    if let Some(variable) = &pattern.variable {
        if bindings.contains_key(variable) {
            return Err(ExecutorError {
                message: format!("variable '{}' is already bound", variable),
            });
        }
    }

    let Some(rel_type) = &pattern.rel_type else {
        return Err(ExecutorError {
            message: "CREATE relationship patterns require a relationship type".to_string(),
        });
    };

    let relationship = rel_repo
        .insert_in_tx(
            transaction,
            rel_type.clone(),
            start_id,
            end_id,
            properties_from_literals(&pattern.properties),
        )
        .await?;

    created_relationships.push(relationship.clone());
    Ok(relationship)
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

fn properties_from_literals(properties: &[crate::parser::Property]) -> Properties {
    properties
        .iter()
        .map(|property| {
            (
                property.key.clone(),
                match &property.value {
                    Literal::String(value) => JsonValue::String(value.clone()),
                    Literal::Number(value) => json_number_from_lexer(value),
                    Literal::Bool(value) => JsonValue::Bool(*value),
                },
            )
        })
        .collect()
}

fn json_number_from_lexer(value: &str) -> JsonValue {
    if value.contains('.') {
        return JsonValue::Number(
            serde_json::Number::from_f64(
                value
                    .parse::<f64>()
                    .expect("lexer should only emit valid floating-point numbers"),
            )
            .expect("finite floating-point number should serialize"),
        );
    }

    JsonValue::Number(
        value
            .parse::<i64>()
            .expect("lexer should only emit valid integer numbers")
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::{execute_create, execute_delete, execute_match_query, Binding, BoundValue};
    use crate::{
        db::{create_pool, NodeRepo, RelRepo},
        domain::Properties,
        graph::GraphIndex,
        parser::{parse, Clause},
    };
    use serde_json::Value as JsonValue;
    use std::collections::BTreeSet;
    use std::sync::OnceLock;
    use tokio::sync::Mutex;

    static DB_TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

    #[tokio::test]
    async fn match_where_return_and_limit_project_rows_from_traversal() {
        let _guard = DB_TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().await;
        let Some(pool) = test_pool().await else {
            return;
        };
        ensure_schema(&pool).await;
        reset_tables(&pool).await;

        let node_repo = NodeRepo::new(pool.clone());
        let rel_repo = RelRepo::new(pool.clone());

        let api = insert_named_node(&node_repo, "Service", "api").await;
        let worker = insert_named_node(&node_repo, "Service", "worker").await;
        let primary = insert_named_node(&node_repo, "Database", "primary").await;
        let replica = insert_named_node(&node_repo, "Database", "replica").await;

        rel_repo
            .insert(
                "DEPENDS_ON".to_string(),
                api.id,
                primary.id,
                properties([("weight", JsonValue::Number(serde_json::Number::from(5)))]),
            )
            .await
            .expect("first relationship insert should succeed");
        rel_repo
            .insert(
                "DEPENDS_ON".to_string(),
                worker.id,
                replica.id,
                properties([("weight", JsonValue::Number(serde_json::Number::from(1)))]),
            )
            .await
            .expect("second relationship insert should succeed");

        let graph_index = GraphIndex::load_from_db(&pool)
            .await
            .expect("graph index should load");
        let query = parse(
            "MATCH (service:Service)-[rel:DEPENDS_ON]->(database:Database) \
             WHERE rel.weight >= 3 \
             RETURN service.name, database.name \
             LIMIT 1",
        )
        .expect("query should parse");

        let summary = execute_match_query(&pool, &graph_index, &query)
            .await
            .expect("match query should succeed");

        assert_eq!(
            summary.columns,
            vec!["service.name".to_string(), "database.name".to_string()]
        );
        assert_eq!(
            summary.rows,
            vec![vec![
                JsonValue::String("api".to_string()),
                JsonValue::String("primary".to_string())
            ]]
        );
        assert_eq!(summary.bindings.len(), 1);
    }

    #[tokio::test]
    async fn match_joins_multiple_patterns_through_shared_variables() {
        let _guard = DB_TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().await;
        let Some(pool) = test_pool().await else {
            return;
        };
        ensure_schema(&pool).await;
        reset_tables(&pool).await;

        let node_repo = NodeRepo::new(pool.clone());
        let rel_repo = RelRepo::new(pool.clone());

        let api = insert_named_node(&node_repo, "Service", "api").await;
        let worker = insert_named_node(&node_repo, "Service", "worker").await;
        let primary = insert_named_node(&node_repo, "Database", "primary").await;
        let fast_cache = insert_named_node(&node_repo, "Cache", "fast").await;
        let cold_cache = insert_named_node(&node_repo, "Cache", "cold").await;

        rel_repo
            .insert(
                "DEPENDS_ON".to_string(),
                api.id,
                primary.id,
                Properties::new(),
            )
            .await
            .expect("depends relationship insert should succeed");
        rel_repo
            .insert(
                "USES".to_string(),
                api.id,
                fast_cache.id,
                Properties::new(),
            )
            .await
            .expect("uses relationship insert should succeed");
        rel_repo
            .insert(
                "USES".to_string(),
                worker.id,
                cold_cache.id,
                Properties::new(),
            )
            .await
            .expect("worker uses relationship insert should succeed");

        let graph_index = GraphIndex::load_from_db(&pool)
            .await
            .expect("graph index should load");
        let query = parse(
            "MATCH (service:Service)-[:DEPENDS_ON]->(database:Database), \
             (service)-[:USES]->(cache:Cache) \
             RETURN service.name, cache.name",
        )
        .expect("query should parse");

        let summary = execute_match_query(&pool, &graph_index, &query)
            .await
            .expect("match query should succeed");

        assert_eq!(summary.rows.len(), 1);
        assert_eq!(
            summary.rows[0],
            vec![
                JsonValue::String("api".to_string()),
                JsonValue::String("fast".to_string())
            ]
        );
    }

    #[tokio::test]
    async fn match_returns_bound_entities_and_filters_with_parenthesized_where() {
        let _guard = DB_TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().await;
        let Some(pool) = test_pool().await else {
            return;
        };
        ensure_schema(&pool).await;
        reset_tables(&pool).await;

        let node_repo = NodeRepo::new(pool.clone());
        let rel_repo = RelRepo::new(pool.clone());

        let api = insert_named_node(&node_repo, "Service", "api").await;
        let primary = insert_named_node(&node_repo, "Database", "primary").await;
        let archive = insert_named_node(&node_repo, "Database", "archive").await;

        rel_repo
            .insert(
                "DEPENDS_ON".to_string(),
                api.id,
                primary.id,
                properties([("weight", JsonValue::Number(serde_json::Number::from(5)))]),
            )
            .await
            .expect("first relationship insert should succeed");
        rel_repo
            .insert(
                "DEPENDS_ON".to_string(),
                api.id,
                archive.id,
                properties([("weight", JsonValue::Number(serde_json::Number::from(2)))]),
            )
            .await
            .expect("second relationship insert should succeed");

        let graph_index = GraphIndex::load_from_db(&pool)
            .await
            .expect("graph index should load");
        let query = parse(
            "MATCH (service:Service)-[rel:DEPENDS_ON]->(database:Database) \
             WHERE (rel.weight > 4 OR database.name = 'archive') AND service.name = 'api' \
             RETURN service, rel, database.name",
        )
        .expect("query should parse");

        let summary = execute_match_query(&pool, &graph_index, &query)
            .await
            .expect("match query should succeed");

        assert_eq!(summary.rows.len(), 2);
        assert!(summary.rows.iter().any(|row| row[2] == JsonValue::String("primary".to_string())));
        assert!(summary.rows.iter().any(|row| row[2] == JsonValue::String("archive".to_string())));
        assert!(summary.rows.iter().all(|row| row[0].get("id").is_some()));
        assert!(summary.rows.iter().all(|row| row[1].get("type").is_some()));
    }

    #[tokio::test]
    async fn create_persists_nodes_relationships_and_bindings() {
        let _guard = DB_TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().await;
        let Some(pool) = test_pool().await else {
            return;
        };
        ensure_schema(&pool).await;
        reset_tables(&pool).await;

        let node_repo = NodeRepo::new(pool.clone());
        let rel_repo = RelRepo::new(pool.clone());
        let mut graph_index = GraphIndex::new();
        let parsed = parse(
            "CREATE (service:Service {name: 'api'})-[depends_on:DEPENDS_ON {weight: 3}]->(database:Database), (database)-[:BACKS]->(cache:Cache)",
        )
        .expect("query should parse");
        let clause = create_clause(&parsed);

        let summary = execute_create(&pool, &mut graph_index, &clause)
            .await
            .expect("create should succeed");

        assert_eq!(summary.created_nodes.len(), 3);
        assert_eq!(summary.created_relationships.len(), 2);
        assert_eq!(
            summary
                .bindings
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>(),
            vec![
                "cache".to_string(),
                "database".to_string(),
                "depends_on".to_string(),
                "service".to_string()
            ]
        );

        let listed_nodes = node_repo.list().await.expect("node list should succeed");
        let listed_relationships = rel_repo.list().await.expect("relationship list should succeed");
        assert_eq!(listed_nodes.len(), 3);
        assert_eq!(listed_relationships.len(), 2);
        assert_eq!(
            listed_nodes[0]
                .properties
                .get("name")
                .expect("name property should exist"),
            &JsonValue::String("api".to_string())
        );
        assert_eq!(
            listed_relationships[0]
                .properties
                .get("weight")
                .expect("weight property should exist"),
            &JsonValue::Number(serde_json::Number::from(3))
        );
        assert_eq!(graph_index.node_count(), 3);
        assert_eq!(graph_index.relationship_count(), 2);
    }

    #[tokio::test]
    async fn create_reuses_previously_created_node_variables_within_statement() {
        let _guard = DB_TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().await;
        let Some(pool) = test_pool().await else {
            return;
        };
        ensure_schema(&pool).await;
        reset_tables(&pool).await;

        let rel_repo = RelRepo::new(pool.clone());
        let mut graph_index = GraphIndex::new();
        let parsed = parse(
            "CREATE (a:Service {name: 'api'})-[:CALLS]->(b:Service {name: 'worker'}), (a)-[:USES]->(c:Database {name: 'primary'})",
        )
        .expect("query should parse");
        let clause = create_clause(&parsed);

        let summary = execute_create(&pool, &mut graph_index, &clause)
            .await
            .expect("create should succeed");

        assert_eq!(summary.created_nodes.len(), 3);
        assert_eq!(summary.created_relationships.len(), 2);
        let relationships = rel_repo.list().await.expect("relationship list should succeed");
        assert_eq!(relationships.len(), 2);
        assert_eq!(relationships[0].start_id, relationships[1].start_id);
    }

    #[tokio::test]
    async fn create_rejects_unresolved_or_redefined_node_references() {
        let _guard = DB_TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().await;
        let Some(pool) = test_pool().await else {
            return;
        };
        ensure_schema(&pool).await;
        reset_tables(&pool).await;

        let mut graph_index = GraphIndex::new();

        let parsed = parse("CREATE (a:Service {name: 'api'}), (a:Database)")
            .expect("query should parse");
        let clause = create_clause(&parsed);
        let error = execute_create(&pool, &mut graph_index, &clause)
            .await
            .expect_err("create should fail");
        assert_eq!(
            error.message,
            "cannot redefine previously-created node variable 'a'"
        );

        let parsed = parse("CREATE (a)-[r]->(b)")
            .expect("query should parse");
        let clause = create_clause(&parsed);
        let error = execute_create(&pool, &mut graph_index, &clause)
            .await
            .expect_err("create should fail");
        assert_eq!(
            error.message,
            "CREATE relationship patterns require a relationship type"
        );
    }

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

    fn create_clause(parsed: &crate::parser::Query) -> crate::parser::CreateClause {
        match &parsed.clauses[0] {
            Clause::Create(clause) => clause.clone(),
            _ => panic!("expected create clause"),
        }
    }

    fn binding<const N: usize>(entries: [(&str, BoundValue); N]) -> Binding {
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect()
    }

    async fn insert_named_node(repo: &NodeRepo, label: &str, name: &str) -> crate::domain::Node {
        repo.insert(
            vec![label.to_string()],
            properties([("name", JsonValue::String(name.to_string()))]),
        )
        .await
        .expect("node insert should succeed")
    }

    fn properties<const N: usize>(entries: [(&str, JsonValue); N]) -> Properties {
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
