//! PLY, ASCII and binary little-endian, with vertex colours.

use super::{Model, ParseError, Result};

pub fn parse(_bytes: &[u8]) -> Result<Model> {
    Err(ParseError::new("not implemented"))
}
