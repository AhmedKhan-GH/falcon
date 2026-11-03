//! The CLIP embedder, through `candle`.

use std::path::Path;

use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::clip::{ClipConfig, ClipModel, div_l2_norm};
use image::DynamicImage;
use tokenizers::Tokenizer;

use crate::embed::{Embedder, IMAGE_SIZE, pixel_values};
use crate::model;

/// CLIP's text encoder reads at most this many tokens.
const MAX_TOKENS: usize = 77;
/// The token that ends a text, and marks where the encoder reads its output.
const END_OF_TEXT: u32 = 49407;
/// The length of the model's vectors.
const DIM: usize = 512;

pub struct ClipEmbedder {
    model: ClipModel,
    tokenizer: Tokenizer,
    device: Device,
}

fn e(err: impl std::fmt::Display) -> String {
    err.to_string()
}

impl ClipEmbedder {
    /// Loads the model from `dir` after checking every file against its
    /// pinned SHA-256. Tries Metal first, and falls back to the CPU if Metal
    /// isn't there or fails a first text.
    pub fn load(dir: &Path) -> Result<ClipEmbedder, String> {
        model::verify(dir)?;
        let (weights, tokenizer) = Self::read(dir)?;
        match Device::new_metal(0).map_err(e).and_then(|device| Self::on(&weights, tokenizer.clone(), device)) {
            Ok(embedder) => Ok(embedder),
            Err(_) => Self::on(&weights, tokenizer, Device::Cpu),
        }
    }

    /// [`load`](Self::load) on the CPU only, to compare speed.
    pub fn load_cpu(dir: &Path) -> Result<ClipEmbedder, String> {
        model::verify(dir)?;
        let (weights, tokenizer) = Self::read(dir)?;
        Self::on(&weights, tokenizer, Device::Cpu)
    }

    fn read(dir: &Path) -> Result<(Vec<u8>, Tokenizer), String> {
        let tokenizer = Tokenizer::from_file(dir.join(model::TOKENIZER)).map_err(e)?;
        // Read rather than mapped, so no `unsafe` rests on the file staying
        // as it is; the buffer goes once the tensors are made.
        let weights = std::fs::read(dir.join(model::WEIGHTS)).map_err(e)?;
        Ok((weights, tokenizer))
    }

    fn on(weights: &[u8], tokenizer: Tokenizer, device: Device) -> Result<ClipEmbedder, String> {
        let vb = VarBuilder::from_slice_safetensors(weights, DType::F32, &device).map_err(e)?;
        let model = ClipModel::new(vb, &ClipConfig::vit_base_patch32()).map_err(e)?;
        let mut embedder = ClipEmbedder { model, tokenizer, device };
        embedder.embed_text("a photo")?;
        Ok(embedder)
    }

    fn rows(features: &Tensor) -> Result<Vec<Vec<f32>>, String> {
        let unit = div_l2_norm(features).map_err(e)?;
        let rows: Vec<Vec<f32>> = unit.to_device(&Device::Cpu).and_then(|t| t.to_vec2()).map_err(e)?;
        if rows.iter().flatten().any(|x| !x.is_finite()) {
            return Err("the model gave an invalid result".to_string());
        }
        Ok(rows)
    }
}

impl Embedder for ClipEmbedder {
    fn embed_images(&mut self, images: &[DynamicImage]) -> Result<Vec<Vec<f32>>, String> {
        if images.is_empty() {
            return Ok(Vec::new());
        }
        let side = IMAGE_SIZE as usize;
        let data: Vec<f32> = images.iter().flat_map(pixel_values).collect();
        let pixels = Tensor::from_vec(data, (images.len(), 3, side, side), &self.device).map_err(e)?;
        let features = self.model.get_image_features(&pixels).map_err(e)?;
        Self::rows(&features)
    }

    fn embed_text(&mut self, text: &str) -> Result<Vec<f32>, String> {
        let encoding = self.tokenizer.encode(text, true).map_err(e)?;
        let mut ids = encoding.get_ids().to_vec();
        if ids.len() > MAX_TOKENS {
            ids.truncate(MAX_TOKENS - 1);
            ids.push(END_OF_TEXT);
        }
        let ids = Tensor::new(ids, &self.device).and_then(|t| t.unsqueeze(0)).map_err(e)?;
        let features = self.model.get_text_features(&ids).map_err(e)?;
        Self::rows(&features)?.pop().ok_or_else(|| "the model gave no result".to_string())
    }

    fn dim(&self) -> usize {
        DIM
    }

    fn device(&self) -> String {
        if self.device.is_metal() { "Metal".to_string() } else { "CPU".to_string() }
    }
}
