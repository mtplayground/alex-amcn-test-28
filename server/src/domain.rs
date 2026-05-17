use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value as JsonValue};

pub type Properties = Map<String, JsonValue>;
pub type NodeId = i64;
pub type RelId = i64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub labels: Vec<String>,
    pub properties: Properties,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Relationship {
    pub id: RelId,
    #[serde(rename = "type")]
    pub r#type: String,
    pub start_id: NodeId,
    pub end_id: NodeId,
    pub properties: Properties,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    String(String),
    Number(Number),
    Bool(bool),
    Null,
}

#[cfg(test)]
mod tests {
    use super::{Node, Relationship, Value};
    use serde_json::{json, Map, Number, Value as JsonValue};

    #[test]
    fn node_serializes_with_expected_shape() {
        let mut properties = Map::new();
        properties.insert("name".to_string(), JsonValue::String("alpha".to_string()));

        let node = Node {
            id: 7,
            labels: vec!["Service".to_string(), "Critical".to_string()],
            properties,
        };

        let serialized = serde_json::to_value(node);
        assert!(serialized.is_ok());

        let serialized = match serialized {
            Ok(serialized) => serialized,
            Err(error) => panic!("node should serialize: {error}"),
        };

        assert_eq!(
            serialized,
            json!({
                "id": 7,
                "labels": ["Service", "Critical"],
                "properties": {
                    "name": "alpha"
                }
            })
        );
    }

    #[test]
    fn relationship_serializes_type_field() {
        let relationship = Relationship {
            id: 9,
            r#type: "DEPENDS_ON".to_string(),
            start_id: 7,
            end_id: 8,
            properties: Map::new(),
        };

        let serialized = serde_json::to_value(relationship);
        assert!(serialized.is_ok());

        let serialized = match serialized {
            Ok(serialized) => serialized,
            Err(error) => panic!("relationship should serialize: {error}"),
        };

        assert_eq!(
            serialized,
            json!({
                "id": 9,
                "type": "DEPENDS_ON",
                "start_id": 7,
                "end_id": 8,
                "properties": {}
            })
        );
    }

    #[test]
    fn value_supports_scalar_variants() {
        let string_value = serde_json::to_value(Value::String("query".to_string()));
        assert!(string_value.is_ok());
        assert_eq!(
            match string_value {
                Ok(serialized) => serialized,
                Err(error) => panic!("string value should serialize: {error}"),
            },
            json!("query")
        );

        let number_value = serde_json::to_value(Value::Number(Number::from(42)));
        assert!(number_value.is_ok());
        assert_eq!(
            match number_value {
                Ok(serialized) => serialized,
                Err(error) => panic!("number value should serialize: {error}"),
            },
            json!(42)
        );

        let bool_value = serde_json::to_value(Value::Bool(true));
        assert!(bool_value.is_ok());
        assert_eq!(
            match bool_value {
                Ok(serialized) => serialized,
                Err(error) => panic!("bool value should serialize: {error}"),
            },
            json!(true)
        );

        let null_value = serde_json::to_value(Value::Null);
        assert!(null_value.is_ok());
        assert_eq!(
            match null_value {
                Ok(serialized) => serialized,
                Err(error) => panic!("null value should serialize: {error}"),
            },
            JsonValue::Null
        );
    }
}
