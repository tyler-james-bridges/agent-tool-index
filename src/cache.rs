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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample_snapshot() -> Snapshot {
        let mut snapshot = Snapshot::empty();
        snapshot.chain_id = 8453;
        snapshot.tool_count = 7;
        snapshot
    }

    #[test]
    fn load_missing_file_returns_empty_snapshot() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("does-not-exist.json");
        let snapshot = load_snapshot(path.to_str().unwrap()).unwrap();
        assert_eq!(snapshot.tool_count, 0);
        assert!(snapshot.tools.is_empty());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("tools.json");
        let path_str = path.to_str().unwrap();

        let original = sample_snapshot();
        save_snapshot(path_str, &original).unwrap();
        let loaded = load_snapshot(path_str).unwrap();

        assert_eq!(loaded.chain_id, 8453);
        assert_eq!(loaded.tool_count, 7);
        assert_eq!(loaded.registry, original.registry);
    }

    #[test]
    fn save_creates_missing_parent_dirs() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested/deep/tools.json");
        let path_str = path.to_str().unwrap();

        save_snapshot(path_str, &sample_snapshot()).unwrap();
        assert!(path.exists());
    }
}
