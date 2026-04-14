//! Module documentation for the sample.

use std::collections::HashMap;
use std::path::Path;

/// A configuration holder.
pub struct Config {
    pub name: String,
    pub values: HashMap<String, String>,
}

/// Private helper enum.
enum Status {
    Active,
    Inactive,
}

pub const MAX_RETRIES: u32 = 3;

/// Process the given input string and return a result.
pub fn process_data(input: &str) -> Result<String, Box<dyn std::error::Error>> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("empty input".into());
    }
    Ok(trimmed.to_uppercase())
}

impl Config {
    /// Create a new Config with the given name.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            values: HashMap::new(),
        }
    }

    fn internal_helper(&self) -> bool {
        !self.values.is_empty()
    }
}

trait Processor {
    fn process(&self, data: &[u8]) -> Vec<u8>;
}
