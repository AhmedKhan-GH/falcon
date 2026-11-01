//! glTF 2.0: binary `.glb` and JSON `.gltf` with its buffers.

use super::{Model, ParseError, Result};

pub fn parse_glb(_bytes: &[u8], _side: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Model> {
    Err(ParseError::new("not implemented"))
}

pub fn parse_gltf(_bytes: &[u8], _side: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Model> {
    Err(ParseError::new("not implemented"))
}
