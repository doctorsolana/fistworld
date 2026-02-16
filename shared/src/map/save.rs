use std::fs;
use std::path::{Path, PathBuf};

use ron::ser::PrettyConfig;

use super::{map_rpath, MapDefinition, MapEditsDefinition};

pub fn map_edits_path(map_dir: &Path) -> PathBuf {
    map_dir.join("edits.ron")
}

pub fn map_definition_path(map_dir: &Path) -> PathBuf {
    map_dir.join("map.ron")
}

pub fn map_dir_for_id(map_id: &str) -> PathBuf {
    map_rpath(map_id)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("maps").join(map_id))
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

pub fn save_map_definition_atomic(path: &Path, definition: &MapDefinition) -> Result<(), String> {
    write_ron_atomic(path, definition)
}

pub fn save_map_edits_atomic(map_dir: &Path, edits: &MapEditsDefinition) -> Result<(), String> {
    edits.validate()?;
    let path = map_edits_path(map_dir);
    write_ron_atomic(&path, edits)
}

fn write_ron_atomic<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("Path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|err| format!("Failed to create directory {}: {err}", parent.display()))?;

    let text = ron::ser::to_string_pretty(
        value,
        PrettyConfig::default()
            .new_line("\n".to_string())
            .indentor("  ".to_string())
            .separate_tuple_members(true)
            .enumerate_arrays(true),
    )
    .map_err(|err| format!("Failed to serialize {}: {err}", path.display()))?;

    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, text.as_bytes())
        .map_err(|err| format!("Failed to write {}: {err}", tmp_path.display()))?;
    fs::rename(&tmp_path, path).map_err(|err| {
        format!(
            "Failed to move {} -> {}: {err}",
            tmp_path.display(),
            path.display()
        )
    })?;

    Ok(())
}
