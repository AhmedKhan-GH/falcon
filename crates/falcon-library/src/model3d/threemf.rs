//! 3MF: a zip holding the model as XML.

use super::{Model, ParseError, Result};

pub fn parse(_bytes: &[u8]) -> Result<Model> {
    Err(ParseError::new("not implemented"))
}
