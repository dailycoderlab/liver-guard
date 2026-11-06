//! Template asset store, content addressed by SHA-256.

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use opencv::prelude::*;
use opencv::core::{Mat, Vector};
use opencv::imgcodecs;
use sha2::{Digest, Sha256};
use liverguard_ir::geometry::py_round;

use crate::util::{resize_by, Result, VisionError};

/// Keeps decoded templates in memory, keyed by hash and resize factor.
///
/// Templates are small, so the cache stays tiny, and caching the resized
/// variant matters: normalization resizes on every match otherwise. Assets are
/// local files only; a missing hash is an error, never a download.
pub struct AssetStore {
    dir: Option<PathBuf>,
    raw: HashMap<String, Rc<Mat>>,
    resized: HashMap<(String, i64), Rc<Mat>>,
}

impl AssetStore {
    pub fn in_memory() -> Self {
        Self { dir: None, raw: HashMap::new(), resized: HashMap::new() }
    }

    pub fn with_dir(dir: impl Into<PathBuf>) -> Self {
        Self { dir: Some(dir.into()), raw: HashMap::new(), resized: HashMap::new() }
    }

    pub fn hash_bytes(payload: &[u8]) -> String {
        hex::encode(Sha256::digest(payload))
    }

    pub fn put_bytes(&mut self, sha: &str, payload: &[u8]) -> Result<()> {
        let buffer = Vector::<u8>::from_slice(payload);
        let image = imgcodecs::imdecode(&buffer, imgcodecs::IMREAD_COLOR)?;
        if image.empty() {
            return Err(VisionError::Asset(format!("asset {} is not a decodable image", short(sha))));
        }
        self.raw.insert(sha.to_string(), Rc::new(image));
        Ok(())
    }

    pub fn put(&mut self, sha: &str, image: Mat) {
        self.raw.insert(sha.to_string(), Rc::new(image));
    }

    pub fn has(&self, sha: &str) -> bool {
        self.raw.contains_key(sha) || self.path(sha).is_some_and(|p| p.exists())
    }

    fn path(&self, sha: &str) -> Option<PathBuf> {
        self.dir.as_ref().map(|dir| dir.join(format!("{sha}.png")))
    }

    pub fn get(&mut self, sha: &str) -> Result<Rc<Mat>> {
        if let Some(image) = self.raw.get(sha) {
            return Ok(image.clone());
        }
        if let Some(path) = self.path(sha) {
            if path.exists() {
                // Decode through OpenCV's buffer API so non-ASCII paths work and
                // pixels match what the matcher was calibrated on.
                let bytes = std::fs::read(&path).map_err(|e| VisionError::Asset(format!("{}: {e}", path.display())))?;
                self.put_bytes(sha, &bytes)?;
                return Ok(self.raw[sha].clone());
            }
        }
        Err(VisionError::Asset(format!("asset {} is not in the local asset directory", short(sha))))
    }

    pub fn get_scaled(&mut self, sha: &str, factor: f64) -> Result<Rc<Mat>> {
        let key = (sha.to_string(), py_round(factor * 1000.0) as i64);
        if let Some(cached) = self.resized.get(&key) {
            return Ok(cached.clone());
        }
        let source = self.get(sha)?;
        let scaled = Rc::new(resize_by(&source, factor)?);
        self.resized.insert(key, scaled.clone());
        Ok(scaled)
    }

    pub fn clear_resized(&mut self) {
        self.resized.clear();
    }
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(8)]
}
