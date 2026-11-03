//! Tests against the real model. They need the model's files and some
//! pictures, so they are `#[ignore]`d; normal `cargo test` uses neither the
//! network nor the weights.
//!
//! `FALCON_CLIP_MODEL` is a folder holding the files in `model::FILES`.
//! `FALCON_CLIP_IMAGES` is a folder of pictures sorted into one folder per
//! class (`cat/1.jpg`, `car/1.jpg`...). The results reported came from
//! CC0 and public-domain pictures on Wikimedia Commons, kept outside the
//! repository.
//!
//! ```sh
//! FALCON_CLIP_MODEL=/tmp/model FALCON_CLIP_IMAGES=/tmp/images \
//!     cargo test -p falcon-clip --release --test real_model -- --ignored --nocapture
//! ```

use std::path::PathBuf;
use std::time::Instant;

use falcon_clip::Embedder;
use falcon_clip::clip::ClipEmbedder;
use image::DynamicImage;

fn model_dir() -> PathBuf {
    PathBuf::from(std::env::var("FALCON_CLIP_MODEL").expect("set FALCON_CLIP_MODEL"))
}

fn images() -> Vec<(String, String, DynamicImage)> {
    let root = PathBuf::from(std::env::var("FALCON_CLIP_IMAGES").expect("set FALCON_CLIP_IMAGES"));
    let mut out = Vec::new();
    let mut classes: Vec<_> = std::fs::read_dir(root).unwrap().flatten().filter(|e| e.path().is_dir()).collect();
    classes.sort_by_key(|e| e.file_name());
    for class in classes {
        let mut files: Vec<_> = std::fs::read_dir(class.path()).unwrap().flatten().collect();
        files.sort_by_key(|e| e.file_name());
        for file in files {
            let image = image::open(file.path()).unwrap();
            out.push((class.file_name().to_string_lossy().into_owned(), file.file_name().to_string_lossy().into_owned(), image));
        }
    }
    out
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn embed_all(clip: &mut ClipEmbedder, pictures: &[(String, String, DynamicImage)]) -> Vec<Vec<f32>> {
    let mut vectors = Vec::new();
    for chunk in pictures.chunks(8) {
        let batch: Vec<DynamicImage> = chunk.iter().map(|p| p.2.clone()).collect();
        vectors.extend(clip.embed_images(&batch).unwrap());
    }
    vectors
}

/// The named test: a picture of a cat ranks above unrelated pictures for
/// "a cat". Also prints how each class does on its own word, and the scores
/// the search cut-offs in `service.rs` were chosen from.
#[test]
#[ignore = "needs the model's files and test pictures"]
fn a_cat_ranks_above_unrelated_pictures() {
    let started = Instant::now();
    let mut clip = ClipEmbedder::load(&model_dir()).unwrap();
    println!("loaded on {} in {:.2}s", clip.device(), started.elapsed().as_secs_f32());
    let pictures = images();
    let vectors = embed_all(&mut clip, &pictures);
    for vector in &vectors {
        assert_eq!(vector.len(), clip.dim());
        assert!((dot(vector, vector) - 1.0).abs() < 1e-3, "vectors are unit length");
    }

    let rank = |clip: &mut ClipEmbedder, query: &str| -> Vec<(f32, usize)> {
        let text = clip.embed_text(query).unwrap();
        let mut scores: Vec<(f32, usize)> = vectors.iter().enumerate().map(|(i, v)| (dot(&text, v), i)).collect();
        scores.sort_by(|a, b| b.0.total_cmp(&a.0));
        scores
    };

    let cats = rank(&mut clip, "a cat");
    println!("query \"a cat\":");
    for (score, i) in cats.iter().take(8) {
        println!("  {score:.3} {}/{}", pictures[*i].0, pictures[*i].1);
    }
    let cat_count = pictures.iter().filter(|p| p.0 == "cat").count();
    assert!(cat_count > 0);
    assert_eq!(pictures[cats[0].1].0, "cat", "the best match for \"a cat\" is a cat");
    let worst_cat = cats.iter().rposition(|(_, i)| pictures[*i].0 == "cat").unwrap() + 1;
    println!("  the {cat_count} cats are all within the top {worst_cat} of {}", pictures.len());

    let classes: std::collections::BTreeSet<String> = pictures.iter().map(|p| p.0.clone()).collect();
    let (mut top1, mut within) = (0, 0);
    for class in &classes {
        let query = format!("a photo of a {class}");
        let ranked = rank(&mut clip, &query);
        let first = &pictures[ranked[0].1].0;
        let count = pictures.iter().filter(|p| &p.0 == class).count();
        let hits = ranked.iter().take(count).filter(|(_, i)| &pictures[*i].0 == class).count();
        let scores: Vec<String> = ranked.iter().take(4).map(|(s, i)| format!("{s:.3} {}", pictures[*i].0)).collect();
        println!("{query:32} top: {first:9} {hits}/{count} in top {count}  [{}]", scores.join(", "));
        top1 += usize::from(first == class);
        within += hits;
    }
    println!("top-1 right for {top1} of {} classes; {within} of {} pictures in their class's top-N", classes.len(), pictures.len());

    println!("picture-to-picture (best other picture per picture):");
    let (mut same, mut others) = (Vec::new(), Vec::new());
    for (i, a) in vectors.iter().enumerate() {
        for (j, b) in vectors.iter().enumerate().skip(i + 1) {
            let score = dot(a, b);
            if pictures[i].0 == pictures[j].0 { same.push(score) } else { others.push(score) }
        }
    }
    let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len().max(1) as f32;
    let max = |v: &[f32]| v.iter().copied().fold(f32::MIN, f32::max);
    println!("  same class mean {:.3}; other classes mean {:.3}, max {:.3}", mean(&same), mean(&others), max(&others));
}

/// Index speed per picture, on Metal (or the fallback) and on the CPU.
#[test]
#[ignore = "needs the model's files and test pictures"]
fn index_speed() {
    let pictures = images();
    let batch: Vec<DynamicImage> = pictures.iter().cycle().take(32).map(|p| p.2.clone()).collect();
    for cpu in [false, true] {
        let started = Instant::now();
        let mut clip = if cpu { ClipEmbedder::load_cpu(&model_dir()).unwrap() } else { ClipEmbedder::load(&model_dir()).unwrap() };
        let loaded = started.elapsed();
        let started = Instant::now();
        for chunk in batch.chunks(falcon_clip::service::BATCH) {
            clip.embed_images(chunk).unwrap();
        }
        let images = started.elapsed();
        let started = Instant::now();
        for _ in 0..8 {
            clip.embed_text("a black cat on a sofa").unwrap();
        }
        let text = started.elapsed();
        println!(
            "{}: load {:.2}s, {:.1} ms per picture (batches of {}), {:.1} ms per text",
            clip.device(),
            loaded.as_secs_f32(),
            images.as_secs_f64() * 1000.0 / batch.len() as f64,
            falcon_clip::service::BATCH,
            text.as_secs_f64() * 1000.0 / 8.0,
        );
    }
}
