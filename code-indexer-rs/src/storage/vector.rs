use anyhow::Result;
use std::path::Path;
use usearch::{Index, IndexOptions, MetricKind, ScalarKind};

pub struct VectorIndex {
    index: Index,
    dimensions: usize,
    index_path: std::path::PathBuf,
}

impl VectorIndex {
    pub fn open(db_dir: &Path, dimensions: usize) -> Result<Self> {
        let index_path = db_dir.join("hnsw.index");
        let options = IndexOptions {
            dimensions,
            metric: MetricKind::Cos,
            quantization: ScalarKind::F32,
            connectivity: 16,
            expansion_add: 128,
            expansion_search: 64,
            multi: false,
        };
        let index = Index::new(&options)?;
        if index_path.exists() {
            index.load(index_path.to_str().unwrap())?;
            // Reserve extra capacity for incremental adds
            let current = index.capacity();
            index.reserve(current + 10_000)?;
        } else {
            index.reserve(10_000)?;
        }
        Ok(Self {
            index,
            dimensions,
            index_path,
        })
    }

    pub fn add(&self, key: u64, vector: &[f32]) -> Result<()> {
        self.index.add(key, vector)?;
        Ok(())
    }

    pub fn remove(&self, key: u64) -> Result<()> {
        self.index.remove(key)?;
        Ok(())
    }

    pub fn search(&self, query: &[f32], limit: usize) -> Result<Vec<(u64, f32)>> {
        if self.index.size() == 0 {
            return Ok(vec![]);
        }
        let results = self.index.search(query, limit)?;
        Ok(results.keys.into_iter().zip(results.distances).collect())
    }

    pub fn save(&self) -> Result<()> {
        self.index.save(self.index_path.to_str().unwrap())?;
        Ok(())
    }

    pub fn size(&self) -> usize {
        self.index.size()
    }

    pub fn dimensions(&self) -> usize {
        self.dimensions
    }

    pub fn needs_rebuild(&self, expected_dimensions: usize) -> bool {
        self.dimensions != expected_dimensions
    }

    pub fn rebuild(db_dir: &Path, dimensions: usize) -> Result<Self> {
        let index_path = db_dir.join("hnsw.index");
        if index_path.exists() {
            std::fs::remove_file(&index_path)?;
        }
        Self::open(db_dir, dimensions)
    }
}
