use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub column: usize,
    pub lexeme: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Identifier(String),
    String(String),
    Number(String),
    Bool(bool),
    Create,
    Match,
    Where,
    Return,
    Delete,
    Detach,
    Limit,
    And,
    Or,
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    LeftBrace,
    RightBrace,
    Dot,
    Colon,
    Comma,
    Dash,
    GreaterThan,
    Equals,
    LessThan,
    NotEquals,
    GreaterThanOrEqual,
    LessThanOrEqual,
    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexerError {
    pub message: String,
    pub line: usize,
    pub column: usize,
}

impl fmt::Display for LexerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at line {}, column {}",
            self.message, self.line, self.column
        )
    }
}

impl std::error::Error for LexerError {}

pub fn lex(input: &str) -> Result<Vec<Token>, LexerError> {
    Lexer::new(input).tokenize()
}

struct Lexer {
    chars: Vec<char>,
    index: usize,
    line: usize,
    column: usize,
}

impl Lexer {
    fn new(input: &str) -> Self {
        Self {
            chars: input.chars().collect(),
            index: 0,
            line: 1,
            column: 1,
        }
    }

    fn tokenize(mut self) -> Result<Vec<Token>, LexerError> {
        let mut tokens = Vec::new();

        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.advance();
                continue;
            }

            let line = self.line;
            let column = self.column;

            let token = match ch {
                '(' => self.single_char_token(TokenKind::LeftParen, line, column),
                ')' => self.single_char_token(TokenKind::RightParen, line, column),
                '[' => self.single_char_token(TokenKind::LeftBracket, line, column),
                ']' => self.single_char_token(TokenKind::RightBracket, line, column),
                '{' => self.single_char_token(TokenKind::LeftBrace, line, column),
                '}' => self.single_char_token(TokenKind::RightBrace, line, column),
                '.' => self.single_char_token(TokenKind::Dot, line, column),
                ':' => self.single_char_token(TokenKind::Colon, line, column),
                ',' => self.single_char_token(TokenKind::Comma, line, column),
                '-' => self.single_char_token(TokenKind::Dash, line, column),
                '=' => self.single_char_token(TokenKind::Equals, line, column),
                '>' => self.greater_than_token(line, column),
                '<' => self.less_than_token(line, column),
                '\'' | '"' => self.string_token(ch, line, column)?,
                ch if ch.is_ascii_digit() => self.number_token(line, column),
                ch if is_identifier_start(ch) => self.identifier_or_keyword(line, column),
                _ => {
                    return Err(LexerError {
                        message: format!("unexpected character '{ch}'"),
                        line,
                        column,
                    });
                }
            };

            tokens.push(token);
        }

        tokens.push(Token {
            kind: TokenKind::Eof,
            line: self.line,
            column: self.column,
            lexeme: String::new(),
        });

        Ok(tokens)
    }

    fn single_char_token(&mut self, kind: TokenKind, line: usize, column: usize) -> Token {
        let lexeme = self.advance().unwrap_or_default().to_string();

        Token {
            kind,
            line,
            column,
            lexeme,
        }
    }

    fn less_than_token(&mut self, line: usize, column: usize) -> Token {
        self.advance();

        if self.peek() == Some('>') {
            self.advance();
            return Token {
                kind: TokenKind::NotEquals,
                line,
                column,
                lexeme: "<>".to_string(),
            };
        }

        if self.peek() == Some('=') {
            self.advance();
            return Token {
                kind: TokenKind::LessThanOrEqual,
                line,
                column,
                lexeme: "<=".to_string(),
            };
        }

        Token {
            kind: TokenKind::LessThan,
            line,
            column,
            lexeme: "<".to_string(),
        }
    }

    fn greater_than_token(&mut self, line: usize, column: usize) -> Token {
        self.advance();

        if self.peek() == Some('=') {
            self.advance();
            return Token {
                kind: TokenKind::GreaterThanOrEqual,
                line,
                column,
                lexeme: ">=".to_string(),
            };
        }

        Token {
            kind: TokenKind::GreaterThan,
            line,
            column,
            lexeme: ">".to_string(),
        }
    }

    fn string_token(
        &mut self,
        quote: char,
        line: usize,
        column: usize,
    ) -> Result<Token, LexerError> {
        self.advance();

        let mut value = String::new();
        let mut lexeme = String::from(quote);

        loop {
            let Some(ch) = self.peek() else {
                return Err(LexerError {
                    message: "unterminated string literal".to_string(),
                    line,
                    column,
                });
            };

            if ch == quote {
                self.advance();
                lexeme.push(quote);
                break;
            }

            if ch == '\\' {
                self.advance();
                lexeme.push('\\');

                let Some(escaped) = self.peek() else {
                    return Err(LexerError {
                        message: "unterminated string escape".to_string(),
                        line: self.line,
                        column: self.column,
                    });
                };

                let resolved = match escaped {
                    '\\' => '\\',
                    '\'' => '\'',
                    '"' => '"',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    other => {
                        return Err(LexerError {
                            message: format!("unsupported escape sequence '\\{other}'"),
                            line: self.line,
                            column: self.column,
                        });
                    }
                };

                self.advance();
                lexeme.push(escaped);
                value.push(resolved);
                continue;
            }

            if ch == '\n' {
                return Err(LexerError {
                    message: "unterminated string literal".to_string(),
                    line,
                    column,
                });
            }

            self.advance();
            lexeme.push(ch);
            value.push(ch);
        }

        Ok(Token {
            kind: TokenKind::String(value),
            line,
            column,
            lexeme,
        })
    }

    fn number_token(&mut self, line: usize, column: usize) -> Token {
        let mut lexeme = String::new();
        let mut seen_dot = false;

        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                lexeme.push(ch);
                self.advance();
                continue;
            }

            if ch == '.' && !seen_dot && self.peek_next().is_some_and(|next| next.is_ascii_digit())
            {
                seen_dot = true;
                lexeme.push(ch);
                self.advance();
                continue;
            }

            break;
        }

        Token {
            kind: TokenKind::Number(lexeme.clone()),
            line,
            column,
            lexeme,
        }
    }

    fn identifier_or_keyword(&mut self, line: usize, column: usize) -> Token {
        let mut lexeme = String::new();

        while let Some(ch) = self.peek() {
            if is_identifier_part(ch) {
                lexeme.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        let uppercase = lexeme.to_ascii_uppercase();
        let kind = match uppercase.as_str() {
            "CREATE" => TokenKind::Create,
            "MATCH" => TokenKind::Match,
            "WHERE" => TokenKind::Where,
            "RETURN" => TokenKind::Return,
            "DELETE" => TokenKind::Delete,
            "DETACH" => TokenKind::Detach,
            "LIMIT" => TokenKind::Limit,
            "AND" => TokenKind::And,
            "OR" => TokenKind::Or,
            "TRUE" => TokenKind::Bool(true),
            "FALSE" => TokenKind::Bool(false),
            _ => TokenKind::Identifier(lexeme.clone()),
        };

        Token {
            kind,
            line,
            column,
            lexeme,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.index).copied()
    }

    fn peek_next(&self) -> Option<char> {
        self.chars.get(self.index + 1).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.index += 1;

        if ch == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }

        Some(ch)
    }
}

fn is_identifier_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_identifier_part(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::{lex, LexerError, TokenKind};

    #[test]
    fn tokenizes_keywords_identifiers_and_punctuation() {
        let tokens = lex("MATCH (person:User)-[rel:KNOWS]->(friend) RETURN person, rel, friend")
            .expect("lexer should tokenize a simple pattern");

        let kinds = tokens
            .into_iter()
            .map(|token| token.kind)
            .collect::<Vec<_>>();

        assert_eq!(
            kinds,
            vec![
                TokenKind::Match,
                TokenKind::LeftParen,
                TokenKind::Identifier("person".to_string()),
                TokenKind::Colon,
                TokenKind::Identifier("User".to_string()),
                TokenKind::RightParen,
                TokenKind::Dash,
                TokenKind::LeftBracket,
                TokenKind::Identifier("rel".to_string()),
                TokenKind::Colon,
                TokenKind::Identifier("KNOWS".to_string()),
                TokenKind::RightBracket,
                TokenKind::Dash,
                TokenKind::GreaterThan,
                TokenKind::LeftParen,
                TokenKind::Identifier("friend".to_string()),
                TokenKind::RightParen,
                TokenKind::Return,
                TokenKind::Identifier("person".to_string()),
                TokenKind::Comma,
                TokenKind::Identifier("rel".to_string()),
                TokenKind::Comma,
                TokenKind::Identifier("friend".to_string()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn tokenizes_literals_and_comparison_operators() {
        let tokens = lex(
            "WHERE n.name = 'Alice' AND n.score >= 10.5 AND n.age <= 99 AND n.active = true AND n.rank <> 0",
        )
        .expect("lexer should tokenize predicates");

        let kinds = tokens
            .into_iter()
            .map(|token| token.kind)
            .collect::<Vec<_>>();

        assert_eq!(
            kinds,
            vec![
                TokenKind::Where,
                TokenKind::Identifier("n".to_string()),
                TokenKind::Dot,
                TokenKind::Identifier("name".to_string()),
                TokenKind::Equals,
                TokenKind::String("Alice".to_string()),
                TokenKind::And,
                TokenKind::Identifier("n".to_string()),
                TokenKind::Dot,
                TokenKind::Identifier("score".to_string()),
                TokenKind::GreaterThanOrEqual,
                TokenKind::Number("10.5".to_string()),
                TokenKind::And,
                TokenKind::Identifier("n".to_string()),
                TokenKind::Dot,
                TokenKind::Identifier("age".to_string()),
                TokenKind::LessThanOrEqual,
                TokenKind::Number("99".to_string()),
                TokenKind::And,
                TokenKind::Identifier("n".to_string()),
                TokenKind::Dot,
                TokenKind::Identifier("active".to_string()),
                TokenKind::Equals,
                TokenKind::Bool(true),
                TokenKind::And,
                TokenKind::Identifier("n".to_string()),
                TokenKind::Dot,
                TokenKind::Identifier("rank".to_string()),
                TokenKind::NotEquals,
                TokenKind::Number("0".to_string()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn preserves_line_and_column_information() {
        let tokens = lex("CREATE\n  (n {name: \"Neo\"})").expect("lexer should track positions");

        assert_eq!(tokens[0].line, 1);
        assert_eq!(tokens[0].column, 1);
        assert_eq!(tokens[1].line, 2);
        assert_eq!(tokens[1].column, 3);
        assert_eq!(tokens[5].line, 2);
        assert_eq!(tokens[5].column, 11);
    }

    #[test]
    fn supports_double_quoted_and_escaped_strings() {
        let tokens = lex("\"line\\nitem\" 'quote\\''").expect("lexer should decode escapes");

        assert_eq!(tokens[0].kind, TokenKind::String("line\nitem".to_string()));
        assert_eq!(tokens[1].kind, TokenKind::String("quote'".to_string()));
        assert_eq!(tokens[2].kind, TokenKind::Eof);
    }

    #[test]
    fn treats_keywords_case_insensitively() {
        let tokens = lex("match detach delete limit or create where return and false")
            .expect("lexer should fold keyword casing");

        let kinds = tokens
            .into_iter()
            .map(|token| token.kind)
            .collect::<Vec<_>>();

        assert_eq!(
            kinds,
            vec![
                TokenKind::Match,
                TokenKind::Detach,
                TokenKind::Delete,
                TokenKind::Limit,
                TokenKind::Or,
                TokenKind::Create,
                TokenKind::Where,
                TokenKind::Return,
                TokenKind::And,
                TokenKind::Bool(false),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn rejects_unterminated_strings() {
        let error = lex("RETURN 'alice").expect_err("lexer should reject unterminated strings");

        assert_eq!(
            error,
            LexerError {
                message: "unterminated string literal".to_string(),
                line: 1,
                column: 8,
            }
        );
    }

    #[test]
    fn rejects_unexpected_characters() {
        let error = lex("@").expect_err("lexer should reject unsupported punctuation");

        assert_eq!(
            error,
            LexerError {
                message: "unexpected character '@'".to_string(),
                line: 1,
                column: 1,
            }
        );
    }
}
