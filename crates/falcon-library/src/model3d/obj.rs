//! Wavefront OBJ, with colours from its MTL library.

use super::{Model, ParseError, Result};

pub fn parse(_bytes: &[u8], _side: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Model> {
    Err(ParseError::new("not implemented"))
}
