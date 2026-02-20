use pest::Parser;
use pest_derive::Parser;
use ots_core::{field_value_to_bool, normalize_name, FieldValue, FormField, FormState};
use thiserror::Error;

#[derive(Parser)]
#[grammar = "grammar.pest"]
struct OtsDslLexer;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Word(String),
    Semicolon,
    Newline,
}

#[derive(Debug, Error)]
pub enum DslError {
    #[error("lexing failed: {0}")]
    Lex(String),
    #[error("unexpected end of input while reading {0}")]
    UnexpectedEof(&'static str),
    #[error("expected label '{expected}', got '{found}'")]
    UnexpectedLabel { expected: String, found: String },
    #[error("invalid integer '{0}'")]
    InvalidInt(String),
    #[error("invalid float '{0}'")]
    InvalidFloat(String),
    #[error("invalid boolean '{0}'")]
    InvalidBool(String),
}

pub struct DslCursor {
    tokens: Vec<Token>,
    idx: usize,
    pub round_to_whole_dollars: bool,
    pub not_app_value: i32,
}

impl DslCursor {
    pub fn new(input: &str) -> Result<Self, DslError> {
        let pairs = OtsDslLexer::parse(Rule::file, input).map_err(|e| DslError::Lex(e.to_string()))?;
        let mut tokens = Vec::new();

        for pair in pairs {
            if pair.as_rule() != Rule::file {
                continue;
            }
            for inner in pair.into_inner() {
                match inner.as_rule() {
                    Rule::word => {
                        let mut w = inner.as_str().to_string();
                        if let Some(stripped) = w.strip_prefix('$') {
                            w = stripped.to_string();
                        }
                        tokens.push(Token::Word(w));
                    }
                    Rule::quoted => {
                        let s = inner.as_str();
                        let unquoted = s.trim_matches('"').replace("\\\"", "\"");
                        tokens.push(Token::Word(unquoted));
                    }
                    Rule::semicolon => tokens.push(Token::Semicolon),
                    Rule::newline => tokens.push(Token::Newline),
                    Rule::comment | Rule::ws => {}
                    _ => {}
                }
            }
        }

        Ok(Self {
            tokens,
            idx: 0,
            round_to_whole_dollars: false,
            not_app_value: 0,
        })
    }

    pub fn set_not_app_value(&mut self, value: i32) {
        self.not_app_value = value;
    }

    fn maybe_consume_pragma(&mut self, include_newlines: bool) -> bool {
        let pos = self.idx;
        match self.tokens.get(self.idx) {
            Some(Token::Word(w)) if w == "Round_to_Whole_Dollars" => {
                self.round_to_whole_dollars = true;
                self.idx += 1;

                if include_newlines {
                    if matches!(self.tokens.get(self.idx), Some(Token::Word(_))) {
                        self.idx += 1;
                    }
                } else {
                    if matches!(self.tokens.get(self.idx), Some(Token::Word(_))) {
                        self.idx += 1;
                    }
                }
                true
            }
            _ => {
                self.idx = pos;
                false
            }
        }
    }

    fn skip_irrelevant(&mut self, include_newlines: bool) {
        loop {
            if self.maybe_consume_pragma(include_newlines) {
                continue;
            }
            match self.tokens.get(self.idx) {
                Some(Token::Newline) if include_newlines => self.idx += 1,
                _ => break,
            }
        }
    }

    fn next_token(&mut self, include_newlines: bool) -> Option<Token> {
        self.skip_irrelevant(include_newlines);
        let tok = self.tokens.get(self.idx).cloned();
        if tok.is_some() {
            self.idx += 1;
        }
        tok
    }

    pub fn expect_label(&mut self, expected: &str) -> Result<(), DslError> {
        let tok = self
            .next_token(true)
            .ok_or(DslError::UnexpectedEof("label"))?;
        match tok {
            Token::Word(found) if found == expected => Ok(()),
            Token::Word(found) => Err(DslError::UnexpectedLabel {
                expected: expected.to_string(),
                found,
            }),
            Token::Semicolon => Err(DslError::UnexpectedLabel {
                expected: expected.to_string(),
                found: ";".to_string(),
            }),
            Token::Newline => Err(DslError::UnexpectedLabel {
                expected: expected.to_string(),
                found: "<newline>".to_string(),
            }),
        }
    }

    pub fn read_int(&mut self) -> Result<i32, DslError> {
        let tok = self.next_token(true).ok_or(DslError::UnexpectedEof("int"))?;
        let s = match tok {
            Token::Word(v) => v,
            Token::Semicolon => return Err(DslError::InvalidInt(";".to_string())),
            Token::Newline => return Err(DslError::InvalidInt("<newline>".to_string())),
        };
        s.parse::<i32>().map_err(|_| DslError::InvalidInt(s))
    }

    pub fn read_float(&mut self) -> Result<f64, DslError> {
        let tok = self
            .next_token(true)
            .ok_or(DslError::UnexpectedEof("float"))?;
        let s = match tok {
            Token::Word(v) => v,
            Token::Semicolon => return Err(DslError::InvalidFloat(";".to_string())),
            Token::Newline => return Err(DslError::InvalidFloat("<newline>".to_string())),
        };
        let mut v = s.parse::<f64>().map_err(|_| DslError::InvalidFloat(s.clone()))?;
        if self.round_to_whole_dollars {
            v = round_like_c(v) as f64;
        }
        Ok(v)
    }

    pub fn read_bool(&mut self) -> Result<i32, DslError> {
        let tok = self
            .next_token(true)
            .ok_or(DslError::UnexpectedEof("bool"))?;
        self.parse_bool_token(tok, false)
    }

    pub fn read_bool_single_line(&mut self) -> Result<i32, DslError> {
        self.skip_irrelevant(false);
        let tok = self.tokens.get(self.idx).cloned();
        match tok {
            None => Ok(self.not_app_value),
            Some(Token::Newline) | Some(Token::Semicolon) => {
                self.idx += 1;
                Ok(self.not_app_value)
            }
            Some(token) => {
                self.idx += 1;
                self.parse_bool_token(token, true)
            }
        }
    }

    fn parse_bool_token(&self, token: Token, single_line_mode: bool) -> Result<i32, DslError> {
        let s = match token {
            Token::Word(v) => v,
            Token::Semicolon if single_line_mode => return Ok(self.not_app_value),
            Token::Newline if single_line_mode => return Ok(self.not_app_value),
            Token::Semicolon => return Err(DslError::InvalidBool(";".to_string())),
            Token::Newline => return Err(DslError::InvalidBool("<newline>".to_string())),
        };

        let upper = s.to_ascii_uppercase();
        if matches!(upper.as_str(), "TRUE" | "YES" | "Y" | "1") {
            return Ok(1);
        }
        if matches!(upper.as_str(), "FALSE" | "NO" | "N" | "0") {
            return Ok(0);
        }
        if upper == "N/A" {
            return Ok(self.not_app_value);
        }

        Err(DslError::InvalidBool(s))
    }

    pub fn read_float_sum_until_semicolon(&mut self) -> Result<f64, DslError> {
        let mut sum = 0.0;
        loop {
            let tok = self.next_token(true).ok_or(DslError::UnexpectedEof("float-list"))?;
            match tok {
                Token::Semicolon => break,
                Token::Newline => continue,
                Token::Word(w) => {
                    let mut v = w
                        .parse::<f64>()
                        .map_err(|_| DslError::InvalidFloat(w.clone()))?;
                    if self.round_to_whole_dollars {
                        v = round_like_c(v) as f64;
                    }
                    sum += v;
                }
            }
        }
        Ok(sum)
    }

    pub fn read_string_until_semicolon(&mut self) -> Result<String, DslError> {
        let mut parts: Vec<String> = Vec::new();
        loop {
            let tok = self.next_token(true).ok_or(DslError::UnexpectedEof("string-list"))?;
            match tok {
                Token::Semicolon => break,
                Token::Newline => continue,
                Token::Word(w) => parts.push(w),
            }
        }
        Ok(parts.join(" "))
    }

    pub fn read_line_text(&mut self) -> String {
        self.skip_irrelevant(false);
        let mut parts = Vec::new();
        while let Some(tok) = self.tokens.get(self.idx).cloned() {
            self.idx += 1;
            match tok {
                Token::Newline => break,
                Token::Semicolon => parts.push(";".to_string()),
                Token::Word(w) => parts.push(w),
            }
        }
        parts.join(" ").trim().to_string()
    }

    pub fn get_line(&mut self, label: &str) -> Result<f64, DslError> {
        self.expect_label(label)?;
        self.read_float_sum_until_semicolon()
    }

    pub fn get_line1(&mut self, label: &str) -> Result<f64, DslError> {
        self.expect_label(label)?;
        self.read_float()
    }

    pub fn get_yes_no(&mut self, label: &str) -> Result<i32, DslError> {
        self.expect_label(label)?;
        self.read_bool()
    }

    pub fn get_yes_no_single_line(&mut self, label: &str) -> Result<i32, DslError> {
        self.expect_label(label)?;
        self.read_bool_single_line()
    }

    pub fn get_line_string(&mut self, label: &str) -> Result<String, DslError> {
        self.expect_label(label)?;
        Ok(self.read_line_text())
    }

    pub fn validate_complete(&mut self) {
        while self.next_token(true).is_some() {}
    }

    pub fn to_form_state(&mut self) -> FormState {
        let mut state = FormState::new();

        while let Some((label, raw_tokens, terminated_by_semicolon)) = self.read_statement() {
            let value = tokens_to_value(&raw_tokens);
            state.push(FormField {
                label,
                value,
                raw_tokens,
                terminated_by_semicolon,
            });
        }

        state
    }

    pub fn parse_form_state(input: &str) -> Result<FormState, DslError> {
        let mut cursor = Self::new(input)?;
        Ok(cursor.to_form_state())
    }

    fn read_statement(&mut self) -> Option<(String, Vec<String>, bool)> {
        self.skip_irrelevant(true);

        let label = loop {
            match self.next_token(true) {
                Some(Token::Word(word)) => break word,
                Some(Token::Semicolon) | Some(Token::Newline) => continue,
                None => return None,
            }
        };

        let mut raw_tokens = Vec::new();
        let mut terminated_by_semicolon = false;

        loop {
            let Some(token) = self.tokens.get(self.idx).cloned() else {
                break;
            };

            match token {
                Token::Semicolon => {
                    self.idx += 1;
                    terminated_by_semicolon = true;
                    break;
                }
                Token::Word(word) => {
                    self.idx += 1;
                    raw_tokens.push(word);
                }
                Token::Newline => {
                    self.idx += 1;

                    while matches!(self.tokens.get(self.idx), Some(Token::Newline)) {
                        self.idx += 1;
                    }

                    let Some(next) = self.tokens.get(self.idx) else {
                        break;
                    };

                    match next {
                        Token::Semicolon => continue,
                        Token::Word(word) if is_label_like(word) => break,
                        Token::Word(_) | Token::Newline => continue,
                    }
                }
            }
        }

        Some((label, raw_tokens, terminated_by_semicolon))
    }
}

fn is_label_like(word: &str) -> bool {
    if word.is_empty() {
        return false;
    }

    if word == "~" {
        return false;
    }

    if word.ends_with(':') || word.contains('_') || word.contains('?') {
        return true;
    }

    let normalized = word.replace(',', "");
    if normalized.parse::<f64>().is_ok() {
        return false;
    }

    word.chars()
        .next()
        .map(|first| first.is_ascii_alphabetic())
        .unwrap_or(false)
}

fn parse_atom(token: &str) -> FieldValue {
    if token == "~" {
        return FieldValue::Null;
    }

    let upper = token.to_ascii_uppercase();
    if matches!(upper.as_str(), "TRUE" | "YES" | "Y" | "1") {
        return FieldValue::Bool(true);
    }
    if matches!(upper.as_str(), "FALSE" | "NO" | "N" | "0") {
        return FieldValue::Bool(false);
    }
    if upper == "N/A" {
        return FieldValue::Null;
    }

    let normalized = token.replace(',', "");
    if let Ok(v) = normalized.parse::<i64>() {
        return FieldValue::Integer(v);
    }
    if let Ok(v) = normalized.parse::<f64>() {
        return FieldValue::Number(v);
    }

    FieldValue::Text(token.to_string())
}

fn tokens_to_value(tokens: &[String]) -> FieldValue {
    match tokens {
        [] => FieldValue::Null,
        [single] => parse_atom(single),
        many => FieldValue::List(many.iter().map(|token| parse_atom(token)).collect()),
    }
}

fn round_like_c(x: f64) -> i64 {
    if x < 0.0 {
        (x - 0.5) as i64
    } else {
        (x + 0.5) as i64
    }
}

pub fn detect_round_to_whole_dollars(state: &FormState) -> bool {
    for entry in state.entries() {
        let label = normalize_name(&entry.label);
        if label.contains("roundtowholedollars") {
            return field_value_to_bool(&entry.value);
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use ots_core::FieldValue;
    use std::fs;

    #[test]
    fn parses_multiline_float_list_with_comments() {
        let input = r#"
L1a
  1000.25 {comment}
  99.75
  ;
"#;
        let mut dsl = DslCursor::new(input).unwrap();
        let value = dsl.get_line("L1a").unwrap();
        assert!((value - 1100.0).abs() < 1e-9);
    }

    #[test]
    fn parses_bool_and_na() {
        let input = "Flag Y\nFlag2 n/a\n";
        let mut dsl = DslCursor::new(input).unwrap();
        dsl.set_not_app_value(7);
        assert_eq!(dsl.get_yes_no_single_line("Flag").unwrap(), 1);
        assert_eq!(dsl.get_yes_no_single_line("Flag2").unwrap(), 7);
    }

    #[test]
    fn parses_line_text() {
        let input = "Your1stName:  Alice Mary {ignored}\n";
        let mut dsl = DslCursor::new(input).unwrap();
        let text = dsl.get_line_string("Your1stName:").unwrap();
        assert_eq!(text, "Alice Mary");
    }

    #[test]
    fn round_pragma_affects_float_reads() {
        let input = "Round_to_Whole_Dollars Y\nL2 1.49 1.49 ;\n";
        let mut dsl = DslCursor::new(input).unwrap();
        let value = dsl.get_line("L2").unwrap();
        assert!((value - 2.0).abs() < 1e-9);
    }

    #[test]
    fn parses_example_us_1040_file() {
        let input = fs::read_to_string("../../test/example_us_1040.txt").unwrap();
        let mut dsl = DslCursor::new(&input).unwrap();

        dsl.expect_label("Title:").unwrap();
        assert_eq!(
            dsl.read_line_text(),
            "US Federal 1040 Tax Form - 2025 -- EXAMPLE"
        );

        dsl.expect_label("Status").unwrap();
        assert_eq!(dsl.read_line_text(), "Married/Joint");

        dsl.expect_label("You_65+Over?").unwrap();
        assert_eq!(dsl.read_bool_single_line().unwrap(), 0);

        dsl.expect_label("You_Blind?").unwrap();
        assert_eq!(dsl.read_bool_single_line().unwrap(), 0);

        dsl.expect_label("Spouse_65+Over?").unwrap();
        assert_eq!(dsl.read_bool_single_line().unwrap(), 1);

        dsl.expect_label("Spouse_Blind?").unwrap();
        assert_eq!(dsl.read_bool_single_line().unwrap(), 0);

        dsl.expect_label("Dependents").unwrap();
        assert_eq!(dsl.read_int().unwrap(), 0);

        dsl.expect_label("CkHomeInUS").unwrap();
        assert_eq!(dsl.read_bool_single_line().unwrap(), 1);

        dsl.expect_label("VirtCurr?").unwrap();
        assert_eq!(dsl.read_bool_single_line().unwrap(), 0);

        dsl.expect_label("CkSepLivedApart").unwrap();
        assert_eq!(dsl.read_bool_single_line().unwrap(), 0);

        assert!((dsl.get_line("L1a").unwrap() - 48456.23).abs() < 1e-9);
    }

    #[test]
    fn parses_entire_example_us_1040_file_to_eof() {
        let input = fs::read_to_string("../../test/example_us_1040.txt").unwrap();
        let mut dsl = DslCursor::new(&input).unwrap();

        while dsl.next_token(true).is_some() {}

        assert_eq!(dsl.idx, dsl.tokens.len());
    }

    #[test]
    fn serializes_example_file_into_form_state() {
        let input = fs::read_to_string("../../test/example_us_1040.txt").unwrap();
        let state = DslCursor::parse_form_state(&input).unwrap();

        assert!(state.len() > 100);

        let status = state.get_first("Status").unwrap();
        assert_eq!(
            status.value,
            FieldValue::Text("Married/Joint".to_string())
        );

        let l1a = state.get_first("L1a").unwrap();
        match &l1a.value {
            FieldValue::List(values) => {
                assert_eq!(values.len(), 2);
                assert_eq!(values[0], FieldValue::Number(20267.70));
                assert_eq!(values[1], FieldValue::Number(28188.53));
            }
            _ => panic!("L1a should serialize as list"),
        }

        let rollovers: Vec<_> = state.get_all("Rollover").collect();
        assert_eq!(rollovers.len(), 2);
        assert_eq!(rollovers[0].value, FieldValue::Null);
    }
}
