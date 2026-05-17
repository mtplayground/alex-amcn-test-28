use std::fmt;

use crate::lexer::{lex, LexerError, Token, TokenKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub clauses: Vec<Clause>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Clause {
    Create(CreateClause),
    Match(MatchClause),
    Where(WhereClause),
    Return(ReturnClause),
    Delete(DeleteClause),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateClause {
    pub patterns: Vec<Pattern>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchClause {
    pub patterns: Vec<Pattern>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhereClause {
    pub expression: Expression,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnClause {
    pub items: Vec<Projection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteClause {
    pub detach: bool,
    pub expressions: Vec<Expression>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Projection {
    pub expression: Expression,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    pub start: NodePattern,
    pub chains: Vec<PatternChain>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternChain {
    pub relationship: RelationshipPattern,
    pub node: NodePattern,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodePattern {
    pub variable: Option<String>,
    pub labels: Vec<String>,
    pub properties: Vec<Property>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationshipPattern {
    pub variable: Option<String>,
    pub rel_type: Option<String>,
    pub properties: Vec<Property>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Property {
    pub key: String,
    pub value: Literal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expression {
    Identifier(String),
    PropertyAccess {
        identifier: String,
        property: String,
    },
    Literal(Literal),
    Binary {
        left: Box<Expression>,
        operator: BinaryOperator,
        right: Box<Expression>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Literal {
    String(String),
    Number(String),
    Bool(bool),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinaryOperator {
    And,
    Or,
    Equals,
    NotEquals,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParserError {
    pub message: String,
    pub line: usize,
    pub column: usize,
}

impl fmt::Display for ParserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at line {}, column {}",
            self.message, self.line, self.column
        )
    }
}

impl std::error::Error for ParserError {}

impl From<LexerError> for ParserError {
    fn from(error: LexerError) -> Self {
        Self {
            message: error.message,
            line: error.line,
            column: error.column,
        }
    }
}

pub fn parse(input: &str) -> Result<Query, ParserError> {
    let tokens = lex(input)?;
    Parser::new(tokens).parse_query()
}

struct Parser {
    tokens: Vec<Token>,
    current: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, current: 0 }
    }

    fn parse_query(&mut self) -> Result<Query, ParserError> {
        let mut clauses = Vec::new();

        while !self.is_at_end() {
            clauses.push(self.parse_clause()?);
        }

        if clauses.is_empty() {
            let token = self.peek();
            return Err(ParserError {
                message: "expected a query clause".to_string(),
                line: token.line,
                column: token.column,
            });
        }

        Ok(Query { clauses })
    }

    fn parse_clause(&mut self) -> Result<Clause, ParserError> {
        if self.match_simple(|kind| matches!(kind, TokenKind::Create)) {
            return Ok(Clause::Create(CreateClause {
                patterns: self.parse_pattern_list()?,
            }));
        }

        if self.match_simple(|kind| matches!(kind, TokenKind::Match)) {
            return Ok(Clause::Match(MatchClause {
                patterns: self.parse_pattern_list()?,
            }));
        }

        if self.match_simple(|kind| matches!(kind, TokenKind::Where)) {
            return Ok(Clause::Where(WhereClause {
                expression: self.parse_expression()?,
            }));
        }

        if self.match_simple(|kind| matches!(kind, TokenKind::Return)) {
            return Ok(Clause::Return(ReturnClause {
                items: self.parse_projection_list()?,
            }));
        }

        let detach = self.match_simple(|kind| matches!(kind, TokenKind::Detach));
        if detach || self.match_simple(|kind| matches!(kind, TokenKind::Delete)) {
            if detach {
                self.expect(
                    |kind| matches!(kind, TokenKind::Delete),
                    "expected DELETE after DETACH",
                )?;
            }

            return Ok(Clause::Delete(DeleteClause {
                detach,
                expressions: self.parse_expression_list()?,
            }));
        }

        let token = self.peek();
        Err(ParserError {
            message: format!("unexpected token '{}'", token.lexeme),
            line: token.line,
            column: token.column,
        })
    }

    fn parse_pattern_list(&mut self) -> Result<Vec<Pattern>, ParserError> {
        let mut patterns = vec![self.parse_pattern()?];

        while self.match_simple(|kind| matches!(kind, TokenKind::Comma)) {
            patterns.push(self.parse_pattern()?);
        }

        Ok(patterns)
    }

    fn parse_pattern(&mut self) -> Result<Pattern, ParserError> {
        let start = self.parse_node_pattern()?;
        let mut chains = Vec::new();

        while self.check(|kind| matches!(kind, TokenKind::Dash)) {
            chains.push(self.parse_pattern_chain()?);
        }

        Ok(Pattern { start, chains })
    }

    fn parse_pattern_chain(&mut self) -> Result<PatternChain, ParserError> {
        self.expect(|kind| matches!(kind, TokenKind::Dash), "expected '-'")?;
        let relationship = self.parse_relationship_pattern()?;
        self.expect(|kind| matches!(kind, TokenKind::Dash), "expected '-'")?;
        self.expect(|kind| matches!(kind, TokenKind::GreaterThan), "expected '>'")?;
        let node = self.parse_node_pattern()?;

        Ok(PatternChain { relationship, node })
    }

    fn parse_node_pattern(&mut self) -> Result<NodePattern, ParserError> {
        self.expect(
            |kind| matches!(kind, TokenKind::LeftParen),
            "expected '(' to start node pattern",
        )?;

        let variable = self.parse_optional_identifier();
        let labels = self.parse_labels()?;
        let properties = if self.check(|kind| matches!(kind, TokenKind::LeftBrace)) {
            self.parse_properties()?
        } else {
            Vec::new()
        };

        self.expect(
            |kind| matches!(kind, TokenKind::RightParen),
            "expected ')' to close node pattern",
        )?;

        Ok(NodePattern {
            variable,
            labels,
            properties,
        })
    }

    fn parse_relationship_pattern(&mut self) -> Result<RelationshipPattern, ParserError> {
        self.expect(
            |kind| matches!(kind, TokenKind::LeftBracket),
            "expected '[' to start relationship pattern",
        )?;

        let variable = self.parse_optional_identifier();
        let rel_type = if self.match_simple(|kind| matches!(kind, TokenKind::Colon)) {
            Some(self.expect_identifier("expected relationship type after ':'")?)
        } else {
            None
        };
        let properties = if self.check(|kind| matches!(kind, TokenKind::LeftBrace)) {
            self.parse_properties()?
        } else {
            Vec::new()
        };

        self.expect(
            |kind| matches!(kind, TokenKind::RightBracket),
            "expected ']' to close relationship pattern",
        )?;

        Ok(RelationshipPattern {
            variable,
            rel_type,
            properties,
        })
    }

    fn parse_labels(&mut self) -> Result<Vec<String>, ParserError> {
        let mut labels = Vec::new();

        while self.match_simple(|kind| matches!(kind, TokenKind::Colon)) {
            labels.push(self.expect_identifier("expected label after ':'")?);
        }

        Ok(labels)
    }

    fn parse_properties(&mut self) -> Result<Vec<Property>, ParserError> {
        self.expect(
            |kind| matches!(kind, TokenKind::LeftBrace),
            "expected '{' to start properties",
        )?;

        let mut properties = Vec::new();
        if self.match_simple(|kind| matches!(kind, TokenKind::RightBrace)) {
            return Ok(properties);
        }

        loop {
            let key = self.expect_identifier("expected property name")?;
            self.expect(
                |kind| matches!(kind, TokenKind::Colon),
                "expected ':' after property name",
            )?;
            let value = self.parse_literal()?;
            properties.push(Property { key, value });

            if self.match_simple(|kind| matches!(kind, TokenKind::Comma)) {
                continue;
            }

            self.expect(
                |kind| matches!(kind, TokenKind::RightBrace),
                "expected '}' to close properties",
            )?;
            break;
        }

        Ok(properties)
    }

    fn parse_projection_list(&mut self) -> Result<Vec<Projection>, ParserError> {
        self.parse_expression_list()
            .map(|expressions| expressions.into_iter().map(|expression| Projection { expression }).collect())
    }

    fn parse_expression_list(&mut self) -> Result<Vec<Expression>, ParserError> {
        let mut expressions = vec![self.parse_expression()?];

        while self.match_simple(|kind| matches!(kind, TokenKind::Comma)) {
            expressions.push(self.parse_expression()?);
        }

        Ok(expressions)
    }

    fn parse_expression(&mut self) -> Result<Expression, ParserError> {
        self.parse_or_expression()
    }

    fn parse_or_expression(&mut self) -> Result<Expression, ParserError> {
        let mut expression = self.parse_and_expression()?;

        while self.match_simple(|kind| matches!(kind, TokenKind::Or)) {
            let right = self.parse_and_expression()?;
            expression = Expression::Binary {
                left: Box::new(expression),
                operator: BinaryOperator::Or,
                right: Box::new(right),
            };
        }

        Ok(expression)
    }

    fn parse_and_expression(&mut self) -> Result<Expression, ParserError> {
        let mut expression = self.parse_comparison_expression()?;

        while self.match_simple(|kind| matches!(kind, TokenKind::And)) {
            let right = self.parse_comparison_expression()?;
            expression = Expression::Binary {
                left: Box::new(expression),
                operator: BinaryOperator::And,
                right: Box::new(right),
            };
        }

        Ok(expression)
    }

    fn parse_comparison_expression(&mut self) -> Result<Expression, ParserError> {
        let mut expression = self.parse_primary_expression()?;

        while let Some(operator) = self.parse_comparison_operator() {
            let right = self.parse_primary_expression()?;
            expression = Expression::Binary {
                left: Box::new(expression),
                operator,
                right: Box::new(right),
            };
        }

        Ok(expression)
    }

    fn parse_primary_expression(&mut self) -> Result<Expression, ParserError> {
        if self.match_simple(|kind| matches!(kind, TokenKind::LeftParen)) {
            let expression = self.parse_expression()?;
            self.expect(
                |kind| matches!(kind, TokenKind::RightParen),
                "expected ')' after expression",
            )?;
            return Ok(expression);
        }

        if let Some(identifier) = self.match_identifier() {
            if self.match_simple(|kind| matches!(kind, TokenKind::Dot)) {
                let property = self.expect_identifier("expected property name after '.'")?;
                return Ok(Expression::PropertyAccess {
                    identifier,
                    property,
                });
            }

            return Ok(Expression::Identifier(identifier));
        }

        Ok(Expression::Literal(self.parse_literal()?))
    }

    fn parse_literal(&mut self) -> Result<Literal, ParserError> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::String(value) => Ok(Literal::String(value)),
            TokenKind::Number(value) => Ok(Literal::Number(value)),
            TokenKind::Bool(value) => Ok(Literal::Bool(value)),
            _ => Err(ParserError {
                message: "expected a literal value".to_string(),
                line: token.line,
                column: token.column,
            }),
        }
    }

    fn parse_comparison_operator(&mut self) -> Option<BinaryOperator> {
        let operator = match &self.peek().kind {
            TokenKind::Equals => BinaryOperator::Equals,
            TokenKind::NotEquals => BinaryOperator::NotEquals,
            TokenKind::LessThan => BinaryOperator::LessThan,
            TokenKind::LessThanOrEqual => BinaryOperator::LessThanOrEqual,
            TokenKind::GreaterThan => BinaryOperator::GreaterThan,
            TokenKind::GreaterThanOrEqual => BinaryOperator::GreaterThanOrEqual,
            _ => return None,
        };

        self.advance();
        Some(operator)
    }

    fn parse_optional_identifier(&mut self) -> Option<String> {
        self.match_identifier()
    }

    fn expect_identifier(&mut self, message: &str) -> Result<String, ParserError> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Identifier(value) => Ok(value),
            _ => Err(ParserError {
                message: message.to_string(),
                line: token.line,
                column: token.column,
            }),
        }
    }

    fn match_identifier(&mut self) -> Option<String> {
        match self.peek().kind.clone() {
            TokenKind::Identifier(value) => {
                self.advance();
                Some(value)
            }
            _ => None,
        }
    }

    fn expect<F>(&mut self, predicate: F, message: &str) -> Result<(), ParserError>
    where
        F: Fn(&TokenKind) -> bool,
    {
        if predicate(&self.peek().kind) {
            self.advance();
            return Ok(());
        }

        let token = self.peek();
        Err(ParserError {
            message: message.to_string(),
            line: token.line,
            column: token.column,
        })
    }

    fn match_simple<F>(&mut self, predicate: F) -> bool
    where
        F: Fn(&TokenKind) -> bool,
    {
        if predicate(&self.peek().kind) {
            self.advance();
            return true;
        }

        false
    }

    fn check<F>(&self, predicate: F) -> bool
    where
        F: Fn(&TokenKind) -> bool,
    {
        predicate(&self.peek().kind)
    }

    fn is_at_end(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
    }

    fn advance(&mut self) -> &Token {
        if !self.is_at_end() {
            self.current += 1;
        }

        self.previous()
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.current]
    }

    fn previous(&self) -> &Token {
        &self.tokens[self.current.saturating_sub(1)]
    }
}

#[cfg(test)]
mod tests {
    use super::{
        parse, BinaryOperator, Clause, Expression, Literal, NodePattern, Pattern, PatternChain,
        Projection, Property, RelationshipPattern,
    };

    #[test]
    fn parses_create_match_where_return_and_delete_clauses() {
        let query = parse(
            "CREATE (person:User {name: 'Alice'})-[rel:KNOWS {since: 2020}]->(friend:User), (friend)-[:LIKES]->(topic:Topic {name: 'Graphs'}) \
             MATCH (person)-[rel:KNOWS]->(friend) \
             WHERE person.name = 'Alice' AND rel.since >= 2020 \
             RETURN person, friend.name, rel \
             DELETE rel",
        )
        .expect("query should parse");

        assert_eq!(
            query.clauses,
            vec![
                Clause::Create(super::CreateClause {
                    patterns: vec![
                        Pattern {
                            start: NodePattern {
                                variable: Some("person".to_string()),
                                labels: vec!["User".to_string()],
                                properties: vec![Property {
                                    key: "name".to_string(),
                                    value: Literal::String("Alice".to_string()),
                                }],
                            },
                            chains: vec![PatternChain {
                                relationship: RelationshipPattern {
                                    variable: Some("rel".to_string()),
                                    rel_type: Some("KNOWS".to_string()),
                                    properties: vec![Property {
                                        key: "since".to_string(),
                                        value: Literal::Number("2020".to_string()),
                                    }],
                                },
                                node: NodePattern {
                                    variable: Some("friend".to_string()),
                                    labels: vec!["User".to_string()],
                                    properties: vec![],
                                },
                            }],
                        },
                        Pattern {
                            start: NodePattern {
                                variable: Some("friend".to_string()),
                                labels: vec![],
                                properties: vec![],
                            },
                            chains: vec![PatternChain {
                                relationship: RelationshipPattern {
                                    variable: None,
                                    rel_type: Some("LIKES".to_string()),
                                    properties: vec![],
                                },
                                node: NodePattern {
                                    variable: Some("topic".to_string()),
                                    labels: vec!["Topic".to_string()],
                                    properties: vec![Property {
                                        key: "name".to_string(),
                                        value: Literal::String("Graphs".to_string()),
                                    }],
                                },
                            }],
                        },
                    ],
                }),
                Clause::Match(super::MatchClause {
                    patterns: vec![Pattern {
                        start: NodePattern {
                            variable: Some("person".to_string()),
                            labels: vec![],
                            properties: vec![],
                        },
                        chains: vec![PatternChain {
                            relationship: RelationshipPattern {
                                variable: Some("rel".to_string()),
                                rel_type: Some("KNOWS".to_string()),
                                properties: vec![],
                            },
                            node: NodePattern {
                                variable: Some("friend".to_string()),
                                labels: vec![],
                                properties: vec![],
                            },
                        }],
                    }],
                }),
                Clause::Where(super::WhereClause {
                    expression: Expression::Binary {
                        left: Box::new(Expression::Binary {
                            left: Box::new(Expression::PropertyAccess {
                                identifier: "person".to_string(),
                                property: "name".to_string(),
                            }),
                            operator: BinaryOperator::Equals,
                            right: Box::new(Expression::Literal(Literal::String(
                                "Alice".to_string(),
                            ))),
                        }),
                        operator: BinaryOperator::And,
                        right: Box::new(Expression::Binary {
                            left: Box::new(Expression::PropertyAccess {
                                identifier: "rel".to_string(),
                                property: "since".to_string(),
                            }),
                            operator: BinaryOperator::GreaterThanOrEqual,
                            right: Box::new(Expression::Literal(Literal::Number(
                                "2020".to_string(),
                            ))),
                        }),
                    },
                }),
                Clause::Return(super::ReturnClause {
                    items: vec![
                        Projection {
                            expression: Expression::Identifier("person".to_string()),
                        },
                        Projection {
                            expression: Expression::PropertyAccess {
                                identifier: "friend".to_string(),
                                property: "name".to_string(),
                            },
                        },
                        Projection {
                            expression: Expression::Identifier("rel".to_string()),
                        },
                    ],
                }),
                Clause::Delete(super::DeleteClause {
                    detach: false,
                    expressions: vec![Expression::Identifier("rel".to_string())],
                }),
            ]
        );
    }

    #[test]
    fn parses_detach_delete_and_parenthesized_predicates() {
        let query = parse(
            "MATCH (n:User) WHERE (n.active = true OR n.score > 10) AND n.name <> 'Bob' RETURN n DETACH DELETE n",
        )
        .expect("query should parse");

        assert_eq!(query.clauses.len(), 4);
        assert_eq!(
            query.clauses[3],
            Clause::Delete(super::DeleteClause {
                detach: true,
                expressions: vec![Expression::Identifier("n".to_string())],
            })
        );
    }

    #[test]
    fn reports_precise_error_positions() {
        let error = parse("CREATE (n {name: })").expect_err("query should fail");

        assert_eq!(error.message, "expected a literal value");
        assert_eq!(error.line, 1);
        assert_eq!(error.column, 18);
    }

    #[test]
    fn surfaces_lexer_errors_from_invalid_input() {
        let error = parse("MATCH (n {name: \"unterminated})").expect_err("query should fail");

        assert_eq!(error.message, "unterminated string literal");
        assert_eq!(error.line, 1);
        assert_eq!(error.column, 17);
    }
}
