use super::ast::*;

#[derive(Debug, PartialEq, Clone)]
enum Token {
    Keyword(String),
    Identifier(String),
    LBrace,
    RBrace,
    Comma,
    Colon,
    AtIndexed,
}

struct Lexer<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Lexer<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() && self.input[self.pos..].chars().next().unwrap().is_whitespace() {
            self.pos += self.input[self.pos..].chars().next().unwrap().len_utf8();
        }
    }

    fn next_token(&mut self) -> Option<Token> {
        self.skip_whitespace();
        if self.pos >= self.input.len() {
            return None;
        }

        let chars: Vec<char> = self.input[self.pos..].chars().collect();
        let c = chars[0];

        if c == '{' {
            self.pos += 1;
            Some(Token::LBrace)
        } else if c == '}' {
            self.pos += 1;
            Some(Token::RBrace)
        } else if c == ',' {
            self.pos += 1;
            Some(Token::Comma)
        } else if c == ':' {
            self.pos += 1;
            Some(Token::Colon)
        } else if c == '@' {
            let mut end = self.pos + 1;
            while end < self.input.len() && self.input[end..].chars().next().unwrap().is_alphanumeric() {
                end += self.input[end..].chars().next().unwrap().len_utf8();
            }
            let word = &self.input[self.pos..end];
            self.pos = end;
            if word == "@indexed" {
                Some(Token::AtIndexed)
            } else {
                self.next_token()
            }
        } else if c.is_alphabetic() || c == '_' {
            let mut end = self.pos;
            while end < self.input.len() {
                let current_char = self.input[end..].chars().next().unwrap();
                if current_char.is_alphanumeric() || current_char == '_' {
                    end += current_char.len_utf8();
                } else {
                    break;
                }
            }
            let word = &self.input[self.pos..end];
            self.pos = end;

            match word {
                "type" | "enum" => Some(Token::Keyword(word.to_string())),
                _ => Some(Token::Identifier(word.to_string())),
            }
        } else {
            // Unexpected char, we can panic or return an error, but let's just skip it for simplicity
            self.pos += c.len_utf8();
            self.next_token()
        }
    }
}

pub struct Parser<'a> {
    lexer: Lexer<'a>,
    current_token: Option<Token>,
}

impl<'a> Parser<'a> {
    pub fn new(input: &'a str) -> Self {
        let mut lexer = Lexer::new(input);
        let current_token = lexer.next_token();
        Self { lexer, current_token }
    }

    fn advance(&mut self) {
        self.current_token = self.lexer.next_token();
    }

    fn expect_identifier(&mut self) -> Result<String, String> {
        match &self.current_token {
            Some(Token::Identifier(id)) => {
                let val = id.clone();
                self.advance();
                Ok(val)
            }
            _ => Err("Expected identifier".to_string()),
        }
    }

    fn match_token(&mut self, expected: Token) -> Result<(), String> {
        if self.current_token == Some(expected.clone()) {
            self.advance();
            Ok(())
        } else {
            Err(format!("Expected {:?}", expected))
        }
    }

    pub fn parse(&mut self) -> Result<Vec<SchemaNode>, String> {
        let mut nodes = Vec::new();
        while self.current_token.is_some() {
            nodes.push(self.parse_node()?);
        }
        Ok(nodes)
    }

    fn parse_node(&mut self) -> Result<SchemaNode, String> {
        match &self.current_token {
            Some(Token::Keyword(k)) if k == "type" => {
                self.advance();
                let name = self.expect_identifier()?;
                self.match_token(Token::LBrace)?;
                let mut fields = Vec::new();
                while self.current_token != Some(Token::RBrace) {
                    let mut is_indexed = false;
                    if self.current_token == Some(Token::AtIndexed) {
                        is_indexed = true;
                        self.advance();
                    }
                    let field_name = self.expect_identifier()?;
                    self.match_token(Token::Colon)?;
                    let type_name = self.expect_identifier()?;
                    let data_type = match type_name.as_str() {
                        "String" => DataType::String,
                        "U16" => DataType::U16,
                        "U32" => DataType::U32,
                        "U64" => DataType::U64,
                        "I32" => DataType::I32,
                        "I64" => DataType::I64,
                        "F64" => DataType::F64,
                        "Boolean" => DataType::Boolean,
                        other => DataType::Custom(other.to_string()),
                    };
                    fields.push(FieldDefinition { name: field_name, data_type, is_indexed });

                    if self.current_token == Some(Token::Comma) {
                        self.advance();
                    } else if self.current_token != Some(Token::RBrace) {
                        return Err("Expected comma or }".to_string());
                    }
                }
                self.match_token(Token::RBrace)?;
                Ok(SchemaNode::Type(TypeDefinition { name, fields }))
            }
            Some(Token::Keyword(k)) if k == "enum" => {
                self.advance();
                let name = self.expect_identifier()?;
                self.match_token(Token::LBrace)?;
                let mut variants = Vec::new();
                while self.current_token != Some(Token::RBrace) {
                    let variant = self.expect_identifier()?;
                    variants.push(variant);

                    if self.current_token == Some(Token::Comma) {
                        self.advance();
                    } else if self.current_token != Some(Token::RBrace) {
                        return Err("Expected comma or }".to_string());
                    }
                }
                self.match_token(Token::RBrace)?;
                Ok(SchemaNode::Enum(EnumDefinition { name, variants }))
            }
            _ => Err("Expected keyword 'type' or 'enum'".to_string()),
        }
    }
}
