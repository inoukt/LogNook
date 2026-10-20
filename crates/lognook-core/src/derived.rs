use crate::{
    LogNookError,
    model::{DerivedSignalDefinition, SignalDefinition},
};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub(crate) struct CompiledDerivedSignal {
    pub definition: DerivedSignalDefinition,
    pub(crate) formula: CompiledFormula,
}

impl CompiledDerivedSignal {
    pub(crate) fn dependencies(&self) -> &HashSet<String> {
        &self.formula.dependencies
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CompiledFormula {
    root: Expr,
    dependencies: HashSet<String>,
}

#[derive(Clone, Debug)]
enum Expr {
    Number(f64),
    Signal(String),
    Positive(Box<Expr>),
    Negative(Box<Expr>),
    Add(Box<Expr>, Box<Expr>),
    Subtract(Box<Expr>, Box<Expr>),
    Multiply(Box<Expr>, Box<Expr>),
    Divide(Box<Expr>, Box<Expr>),
}

impl CompiledFormula {
    fn parse(input: &str) -> Result<Self, LogNookError> {
        let mut parser = Parser {
            input: input.as_bytes(),
            position: 0,
            dependencies: HashSet::new(),
        };
        let root = parser.expression()?;
        parser.skip_whitespace();
        if parser.position != parser.input.len() {
            return Err(parser.error("Unexpected input after formula"));
        }
        if parser.dependencies.is_empty() {
            return Err(LogNookError::new(
                "A derived formula must reference at least one signal",
            ));
        }
        Ok(Self {
            root,
            dependencies: parser.dependencies,
        })
    }

    pub(crate) fn evaluate(&self, values: &HashMap<String, f64>) -> Result<f64, LogNookError> {
        let result = self.root.evaluate(values)?;
        if !result.is_finite() {
            return Err(LogNookError::new("Derived formula result must be finite"));
        }
        Ok(result)
    }
}

impl Expr {
    fn evaluate(&self, values: &HashMap<String, f64>) -> Result<f64, LogNookError> {
        let value = match self {
            Self::Number(value) => *value,
            Self::Signal(id) => *values
                .get(id)
                .ok_or_else(|| LogNookError::new(format!("No current sample for signal '{id}'")))?,
            Self::Positive(expression) => expression.evaluate(values)?,
            Self::Negative(expression) => -expression.evaluate(values)?,
            Self::Add(left, right) => left.evaluate(values)? + right.evaluate(values)?,
            Self::Subtract(left, right) => left.evaluate(values)? - right.evaluate(values)?,
            Self::Multiply(left, right) => left.evaluate(values)? * right.evaluate(values)?,
            Self::Divide(left, right) => {
                let divisor = right.evaluate(values)?;
                if divisor == 0.0 {
                    return Err(LogNookError::new("Derived formula cannot divide by zero"));
                }
                left.evaluate(values)? / divisor
            }
        };
        if !value.is_finite() {
            return Err(LogNookError::new("Derived formula result must be finite"));
        }
        Ok(value)
    }
}

pub(crate) fn compile_derived_signals(
    definitions: &[DerivedSignalDefinition],
    catalog: &[SignalDefinition],
) -> Result<Vec<CompiledDerivedSignal>, LogNookError> {
    let known_ids = catalog
        .iter()
        .map(|signal| signal.id.as_str())
        .collect::<HashSet<_>>();
    let mut seen = HashSet::new();
    let mut compiled = Vec::with_capacity(definitions.len());
    for definition in definitions {
        if !valid_id(&definition.id)
            || definition.name.trim().is_empty()
            || definition.name.len() > 100
        {
            return Err(LogNookError::new(
                "Derived signals need a valid id and a name",
            ));
        }
        if known_ids.contains(definition.id.as_str()) || !seen.insert(definition.id.as_str()) {
            return Err(LogNookError::new(format!(
                "Derived signal id '{}' is already in use",
                definition.id
            )));
        }
        if definition.expression.trim().is_empty() || definition.expression.len() > 512 {
            return Err(LogNookError::new(format!(
                "Formula for '{}' must be 1–512 characters",
                definition.id
            )));
        }
        if definition.unit.trim().is_empty() || definition.unit.len() > 32 {
            return Err(LogNookError::new(format!(
                "Unit for '{}' must be 1–32 characters",
                definition.id
            )));
        }
        if !definition.tare_offset.is_finite() {
            return Err(LogNookError::new(format!(
                "Tare offset for '{}' must be finite",
                definition.id
            )));
        }
        let formula = CompiledFormula::parse(&definition.expression)?;
        for dependency in &formula.dependencies {
            if !known_ids.contains(dependency.as_str()) {
                return Err(LogNookError::new(format!(
                    "Formula for '{}' references unknown signal '{dependency}'",
                    definition.id
                )));
            }
        }
        compiled.push(CompiledDerivedSignal {
            definition: definition.clone(),
            formula,
        });
    }
    Ok(compiled)
}

fn valid_id(id: &str) -> bool {
    let mut bytes = id.bytes();
    bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

struct Parser<'a> {
    input: &'a [u8],
    position: usize,
    dependencies: HashSet<String>,
}

impl Parser<'_> {
    fn expression(&mut self) -> Result<Expr, LogNookError> {
        let mut expression = self.term()?;
        loop {
            self.skip_whitespace();
            expression = match self.peek() {
                Some(b'+') => {
                    self.position += 1;
                    Expr::Add(Box::new(expression), Box::new(self.term()?))
                }
                Some(b'-') => {
                    self.position += 1;
                    Expr::Subtract(Box::new(expression), Box::new(self.term()?))
                }
                _ => return Ok(expression),
            };
        }
    }

    fn term(&mut self) -> Result<Expr, LogNookError> {
        let mut expression = self.unary()?;
        loop {
            self.skip_whitespace();
            expression = match self.peek() {
                Some(b'*') => {
                    self.position += 1;
                    Expr::Multiply(Box::new(expression), Box::new(self.unary()?))
                }
                Some(b'/') => {
                    self.position += 1;
                    Expr::Divide(Box::new(expression), Box::new(self.unary()?))
                }
                _ => return Ok(expression),
            };
        }
    }

    fn unary(&mut self) -> Result<Expr, LogNookError> {
        self.skip_whitespace();
        match self.peek() {
            Some(b'+') => {
                self.position += 1;
                Ok(Expr::Positive(Box::new(self.unary()?)))
            }
            Some(b'-') => {
                self.position += 1;
                Ok(Expr::Negative(Box::new(self.unary()?)))
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Result<Expr, LogNookError> {
        self.skip_whitespace();
        match self.peek() {
            Some(b'(') => {
                self.position += 1;
                let expression = self.expression()?;
                self.skip_whitespace();
                if self.peek() != Some(b')') {
                    return Err(self.error("Expected ')'"));
                }
                self.position += 1;
                Ok(expression)
            }
            Some(byte) if byte.is_ascii_digit() || byte == b'.' => self.number(),
            Some(byte) if byte.is_ascii_alphabetic() || byte == b'_' => self.signal(),
            _ => Err(self.error("Expected a number, signal id, or parenthesized formula")),
        }
    }

    fn number(&mut self) -> Result<Expr, LogNookError> {
        let start = self.position;
        let mut digits = 0;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.position += 1;
            digits += 1;
        }
        if self.peek() == Some(b'.') {
            self.position += 1;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.position += 1;
                digits += 1;
            }
        }
        if digits == 0 {
            return Err(self.error("Expected digits in number"));
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.position += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.position += 1;
            }
            let exponent_start = self.position;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.position += 1;
            }
            if exponent_start == self.position {
                return Err(self.error("Expected exponent digits"));
            }
        }
        let text = std::str::from_utf8(&self.input[start..self.position]).unwrap_or("");
        let value = text
            .parse::<f64>()
            .map_err(|_| self.error("Invalid number"))?;
        if !value.is_finite() {
            return Err(self.error("Formula numbers must be finite"));
        }
        Ok(Expr::Number(value))
    }

    fn signal(&mut self) -> Result<Expr, LogNookError> {
        let start = self.position;
        self.position += 1;
        while self
            .peek()
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            self.position += 1;
        }
        let id = String::from_utf8(self.input[start..self.position].to_vec()).unwrap_or_default();
        self.dependencies.insert(id.clone());
        Ok(Expr::Signal(id))
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.position += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.position).copied()
    }

    fn error(&self, message: &str) -> LogNookError {
        LogNookError::new(format!(
            "{message} at formula position {}",
            self.position + 1
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::CompiledFormula;
    use std::collections::HashMap;

    #[test]
    fn expression_evaluation_uses_arithmetic_precedence_and_parentheses() {
        let formula = CompiledFormula::parse("coolant_temp * 1.8 + 32").unwrap();
        assert_eq!(
            formula
                .evaluate(&HashMap::from([("coolant_temp".into(), 20.0)]))
                .unwrap(),
            68.0
        );
        assert_eq!(
            CompiledFormula::parse("-(engine_rpm - 2) / 2")
                .unwrap()
                .evaluate(&HashMap::from([("engine_rpm".into(), 10.0)]))
                .unwrap(),
            -4.0
        );
    }

    #[test]
    fn expression_errors_cover_unknown_inputs_divide_by_zero_and_non_finite_results() {
        let empty = HashMap::new();
        assert!(
            CompiledFormula::parse("unknown_signal + 1")
                .unwrap()
                .evaluate(&empty)
                .unwrap_err()
                .to_string()
                .contains("unknown_signal")
        );
        let values = HashMap::from([("engine_rpm".into(), 2.0)]);
        assert!(
            CompiledFormula::parse("engine_rpm / (2 - 2)")
                .unwrap()
                .evaluate(&values)
                .unwrap_err()
                .to_string()
                .contains("zero")
        );
        assert!(
            CompiledFormula::parse("engine_rpm * 1e308 * 1e308")
                .unwrap()
                .evaluate(&values)
                .unwrap_err()
                .to_string()
                .contains("finite")
        );
        assert!(CompiledFormula::parse("(engine_rpm +").is_err());
    }
}
