//! Baked static collider database types.
//!
//! The offline `collider_baker` tool writes `colliders.bin` using these types.
//! The server will load it to spawn static colliders for world props.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Versioned database of baked colliders, keyed by `PropKind::id()` (stable string id).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BakedColliderDb {
    pub version: u32,
    pub entries: HashMap<String, BakedCollider>,
}

/// A baked collider shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BakedCollider {
    /// Convex hull defined by a set of points (typically hull vertices).
    ConvexHull { points: Vec<[f32; 3]> },
    /// Compound collider made of multiple convex hulls.
    CompoundConvex { hulls: Vec<Vec<[f32; 3]>> },
}

/// Load a baked collider DB from bytes (bincode).
pub fn load_baked_collider_db_from_bytes(bytes: &[u8]) -> Result<BakedColliderDb, String> {
    let db: BakedColliderDb =
        bincode::deserialize(bytes).map_err(|e| format!("bincode deserialize failed: {e}"))?;
    if db.version != 1 {
        return Err(format!(
            "Unsupported BakedColliderDb version {} (expected 1)",
            db.version
        ));
    }
    Ok(db)
}

/// Load a baked collider DB from a file path (bincode).
pub fn load_baked_collider_db_from_file(path: impl AsRef<Path>) -> Result<BakedColliderDb, String> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|e| format!("failed to read {path:?}: {e}"))?;
    load_baked_collider_db_from_bytes(&bytes)
}
