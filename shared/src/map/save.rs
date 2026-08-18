use std::fs;
use std::path::{Path, PathBuf};

use super::MapEditsDefinition;

pub fn map_edits_path(map_dir: &Path) -> PathBuf {
    map_dir.join("edits.ron")
}

pub fn load_map_edits_optional(map_dir: &Path) -> Result<Option<MapEditsDefinition>, String> {
    let path = map_edits_path(map_dir);
    if !path.exists() {
        return Ok(None);
    }

    let bytes =
        fs::read(&path).map_err(|err| format!("Failed to read {}: {err}", path.display()))?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|err| format!("{} is not valid UTF-8: {err}", path.display()))?;
    let edits: MapEditsDefinition =
        ron::from_str(text).map_err(|err| format!("Failed to parse {}: {err}", path.display()))?;
    edits
        .validate()
        .map_err(|err| format!("Invalid {}: {err}", path.display()))?;
    Ok(Some(edits))
}
