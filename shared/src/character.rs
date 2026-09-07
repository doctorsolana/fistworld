//! Character manifest: the wardrobe + animation contract for the player model.
//!
//! `client/assets/characters/Humanoid.ron` is EMITTED BY THE ART BUILD
//! (asset_creation/build_wardrobe_v2.py) alongside the glb, so node names,
//! slot contents, skin tones and clip names cannot drift from the asset.
//! Nothing in the game may hardcode those names — enumerate them from here.
//!
//! It lives in `shared` rather than the client because outfit choices are
//! replicated: the server validates indices against the same manifest the
//! client renders from.

use serde::Deserialize;
pub mod locomotion;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// A worn item's explicit visibility exclusions (for example helmet -> hair).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct CharacterCoverage {
    pub item: String,
    pub hides_slots: Vec<String>,
}

/// Named equipment recipe; unspecified slots preserve the person's appearance.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct CharacterOutfitPreset {
    pub name: String,
    pub items: BTreeMap<String, String>,
}

/// One wardrobe slot (e.g. "hair"). Exactly ONE item per slot may be visible:
/// two overlapping garments read as an untextured patch where they intersect
/// (CHARACTER_PIPELINE.md records this from v1).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct CharacterSlot {
    /// Slot id, e.g. "bottom" / "top" / "hair".
    pub name: String,
    /// Node name worn when nothing is chosen.
    pub default: String,
    /// Node names of every item that can fill this slot.
    pub items: Vec<String>,
}

impl CharacterSlot {
    /// Index of [`Self::default`] within [`Self::items`], or 0.
    pub fn default_index(&self) -> u8 {
        self.items
            .iter()
            .position(|item| item == &self.default)
            .unwrap_or(0) as u8
    }

    /// Item node worn for a (possibly out-of-range) replicated index.
    pub fn item(&self, index: u8) -> Option<&str> {
        if self.items.is_empty() {
            return None;
        }
        let index = (index as usize).min(self.items.len() - 1);
        Some(self.items[index].as_str())
    }
}

/// One selectable skin tone.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct SkinTone {
    /// Display name, e.g. "Porcelain".
    pub name: String,
    /// Linear RGB.
    pub rgb: (f32, f32, f32),
}

/// Skin is a MATERIAL RECOLOUR, not a mesh swap: only the tone the body ships
/// wearing reaches the glb (glTF drops unused materials), so the client clones
/// [`Self::material`] per hero and re-points its base colour.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct CharacterSkin {
    /// Material in the glb to re-point, e.g. "Skin_Tan".
    pub material: String,
    /// Tone name the body ships wearing.
    pub default: String,
    pub tones: Vec<SkinTone>,
}

/// The full character contract.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct CharacterManifest {
    pub version: u32,
    /// Asset path of the scene, e.g. "characters/Humanoid.glb#Scene0".
    pub scene: String,
    /// Body mesh node name; wears the skin material.
    pub body: String,
    pub slots: Vec<CharacterSlot>,
    #[serde(default)]
    pub coverage: Vec<CharacterCoverage>,
    #[serde(default)]
    pub outfits: Vec<CharacterOutfitPreset>,
    pub skin: CharacterSkin,
    /// Clips that drive the body skeleton (idle/walk/...).
    pub body_clips: Vec<String>,
    /// Clips that drive the face only (expressions).
    pub face_clips: Vec<String>,
}

/// Where the manifest sits relative to an asset root.
const MANIFEST_RELATIVE: &str = "characters/Humanoid.ron";

impl CharacterManifest {
    /// Apply a data-named kit while retaining skin tone, hair and omitted slots.
    /// Resolve the complete recipe before modifying the replicated outfit.
    pub fn apply_outfit(
        &self,
        name: &str,
        outfit: &mut crate::components::HeroOutfit,
    ) -> Result<(), String> {
        let preset = self
            .outfits
            .iter()
            .find(|preset| preset.name == name)
            .ok_or_else(|| format!("unknown character outfit '{name}'"))?;
        let mut next = *outfit;
        for (slot_name, item) in &preset.items {
            let index = self
                .slot_index(slot_name)
                .ok_or_else(|| format!("unknown slot '{slot_name}'"))?;
            let item_index = self.slots[index]
                .items
                .iter()
                .position(|candidate| candidate == item)
                .ok_or_else(|| format!("unknown item '{item}' in '{slot_name}'"))?;
            next.slots[index] = item_index as u8;
        }
        *outfit = next;
        Ok(())
    }
    /// Load the shipped manifest, searching the same asset roots as maps.
    pub fn load() -> Result<Self, String> {
        let (path, text) = read_manifest_text()?;
        let manifest: Self =
            ron::from_str(&text).map_err(|e| format!("failed to parse {}: {e}", path.display()))?;
        manifest
            .validate()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(manifest)
    }

    /// Structural checks that catch an asset/manifest mismatch at startup
    /// rather than as an invisible character in game.
    pub fn validate(&self) -> Result<(), String> {
        if self.slots.is_empty() {
            return Err("manifest has no wardrobe slots".to_string());
        }
        if self.slots.len() > crate::components::HERO_SLOT_MAX {
            return Err("wardrobe exceeds replicated slot capacity".into());
        }
        for slot in &self.slots {
            if slot.items.is_empty() {
                return Err(format!("slot '{}' has no items", slot.name));
            }
            if slot.items.len() > 256 {
                return Err(format!("slot '{}' exceeds u8 item capacity", slot.name));
            }
            if !slot.items.contains(&slot.default) {
                return Err(format!(
                    "slot '{}' default '{}' is not one of its items",
                    slot.name, slot.default
                ));
            }
        }
        for rule in &self.coverage {
            if !self.is_wardrobe_node(&rule.item)
                || rule
                    .hides_slots
                    .iter()
                    .any(|slot| self.slot_index(slot).is_none())
            {
                return Err(format!("invalid wardrobe coverage for '{}'", rule.item));
            }
        }
        for preset in &self.outfits {
            self.apply_outfit(&preset.name, &mut crate::components::HeroOutfit::default())?;
        }
        if self.skin.tones.is_empty() {
            return Err("manifest has no skin tones".to_string());
        }
        if !self.skin.tones.iter().any(|t| t.name == self.skin.default) {
            return Err(format!(
                "default skin '{}' is not one of the tones",
                self.skin.default
            ));
        }
        if self.body_clips.is_empty() {
            return Err("manifest has no body clips".to_string());
        }
        Ok(())
    }

    /// Slot index by name, for UI ordering that must not depend on file order.
    pub fn slot_index(&self, name: &str) -> Option<usize> {
        self.slots.iter().position(|slot| slot.name == name)
    }

    /// Index of the tone the body ships wearing, or 0.
    pub fn default_skin_index(&self) -> u8 {
        self.skin
            .tones
            .iter()
            .position(|tone| tone.name == self.skin.default)
            .unwrap_or(0) as u8
    }

    /// Tone for a (possibly out-of-range) replicated index; clamped.
    pub fn skin_tone(&self, index: u8) -> Option<&SkinTone> {
        if self.skin.tones.is_empty() {
            return None;
        }
        let index = (index as usize).min(self.skin.tones.len() - 1);
        self.skin.tones.get(index)
    }

    /// Linear RGB for a (possibly out-of-range) replicated tone index.
    pub fn skin_color(&self, index: u8) -> [f32; 3] {
        self.skin_tone(index)
            .map(|tone| [tone.rgb.0, tone.rgb.1, tone.rgb.2])
            .unwrap_or([0.807, 0.337, 0.117])
    }

    /// True when `node` is a wardrobe item of any slot — i.e. the dresser owns
    /// its visibility. Exact match only: glTF primitive children are suffixed
    /// ("Hair_Afro.0") and hiding those would override their shown parent.
    pub fn is_wardrobe_node(&self, node: &str) -> bool {
        self.slots
            .iter()
            .any(|slot| slot.items.iter().any(|item| item == node))
    }
}

fn read_manifest_text() -> Result<(PathBuf, String), String> {
    let mut tried = Vec::new();
    for root in crate::map::asset_roots() {
        let path = root.join(MANIFEST_RELATIVE);
        match std::fs::read_to_string(&path) {
            Ok(text) => return Ok((path, text)),
            Err(_) => tried.push(path),
        }
    }
    Err(format!(
        "character manifest not found; tried: {}",
        tried
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equipment_presets_cover_hair_without_losing_the_selected_style() {
        let manifest = CharacterManifest::load().unwrap();
        let mut outfit = crate::components::HeroOutfit::from_manifest(&manifest);
        let hair = manifest.slot_index("hair").unwrap();
        outfit.slots[hair] = 5;
        let before = outfit;
        manifest.apply_outfit("soldier_mail", &mut outfit).unwrap();
        assert_eq!(outfit.slots[hair], before.slots[hair]);
        assert_eq!(outfit.skin, before.skin);
        assert!(outfit.hides_node(&manifest, "Hair_Afro"));
        assert!(!outfit.hides_node(&manifest, "Headgear_NasalHelmet"));
        outfit.slots[manifest.slot_index("headgear").unwrap()] = 0;
        assert!(!outfit.hides_node(&manifest, "Hair_Afro"));
        let before = outfit;
        assert!(manifest.apply_outfit("missing", &mut outfit).is_err());
        assert_eq!(outfit, before);
    }

    #[test]
    fn civilian_variation_does_not_start_wearing_new_armour() {
        for seed in 0..100 {
            let outfit = crate::components::HeroOutfit::varied(seed);
            assert!(outfit.slots[0] < 4 && outfit.slots[1] < 4);
            assert_eq!(outfit.slots[3..], [0, 0, 0]);
        }
    }

    /// The shipped manifest must parse and be self-consistent. This is the
    /// guard against an art build that renames a node without the game
    /// noticing — it fails here instead of rendering a bald, naked hero.
    #[test]
    fn shipped_manifest_is_valid() {
        let manifest = CharacterManifest::load().expect("shipped manifest loads");
        assert_eq!(manifest.version, 1);
        assert_eq!(manifest.scene, "characters/Humanoid.glb#Scene0");

        let glb_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../client/assets/characters/Humanoid.glb");
        let bytes = std::fs::read(&glb_path).expect("shipped humanoid GLB exists");
        let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let document: serde_json::Value =
            serde_json::from_slice(&bytes[20..20 + json_len]).expect("humanoid GLB JSON parses");
        assert_eq!(document["scenes"][0]["name"], "Humanoid");
        let animation_names: Vec<_> = document["animations"]
            .as_array()
            .expect("humanoid animations array")
            .iter()
            .filter_map(|animation| animation["name"].as_str())
            .collect();
        for clip in manifest.body_clips.iter().chain(&manifest.face_clips) {
            assert!(
                animation_names.contains(&clip.as_str()),
                "manifest clip '{clip}' is missing from the shipped humanoid"
            );
        }
        assert!(manifest.body_clips.iter().any(|clip| clip == "pull"));
        assert!(manifest.slot_index("hair").is_some());
        // Every slot fits the replicated component's capacity.
        assert!(
            manifest.slots.len() <= crate::components::HERO_SLOT_MAX,
            "manifest has {} slots but HeroOutfit carries {}; raise \
             HERO_SLOT_MAX (it is replicated — bump both sides together)",
            manifest.slots.len(),
            crate::components::HERO_SLOT_MAX
        );
        // The default look must resolve to real items, not fall back to 0.
        let default_outfit = crate::components::HeroOutfit::from_manifest(&manifest);
        for (index, slot) in manifest.slots.iter().enumerate() {
            assert_eq!(
                slot.item(default_outfit.slot(index)),
                Some(slot.default.as_str())
            );
        }
        assert_eq!(
            manifest
                .skin_tone(default_outfit.skin)
                .map(|t| t.name.as_str()),
            Some(manifest.skin.default.as_str())
        );
    }

    #[test]
    fn slot_lookups_clamp_out_of_range_indices() {
        let slot = CharacterSlot {
            name: "hair".to_string(),
            default: "B".to_string(),
            items: vec!["A".to_string(), "B".to_string()],
        };
        assert_eq!(slot.default_index(), 1);
        assert_eq!(slot.item(0), Some("A"));
        // A hostile or stale client must never index out of bounds.
        assert_eq!(slot.item(200), Some("B"));
    }
}
