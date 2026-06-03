use std::fs;
use std::path::Path;

use anyhow::Result;

use crate::types::Snapshot;

pub fn load_snapshot(path: &str) -> Result<Snapshot> {
    if !Path::new(path).exists() {
        return Ok(Snapshot::empty());
    }
    let data = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&data)?)
}

pub fn save_snapshot(path: &str, snapshot: &Snapshot) -> Result<()> {
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_string_pretty(snapshot)?;
    fs::write(path, data)?;
    Ok(())
}
