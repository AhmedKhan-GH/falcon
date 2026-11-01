//! STL, ASCII and binary.

use super::{Model, ParseError, Result};

pub fn parse(_bytes: &[u8]) -> Result<Model> {
    Err(ParseError::new("not implemented"))
}
