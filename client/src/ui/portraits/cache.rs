//! Byte-bounded output textures. Visible keys are pinned, with deduplicated
//! requests and one worker shared by every HUD/book portrait.

use bevy::{
    asset::RenderAssetUsages,
    platform::collections::{HashMap, HashSet},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    tasks::Task,
};
use shared::components::{HeroOutfit, PersonId, HERO_SLOT_MAX};

pub(super) const TEXTURE_BUDGET: usize = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct Key {
    slots: [u8; HERO_SLOT_MAX],
    skin: u8,
    pub size: u32,
    /// Includes source revision and world session, so an old job cannot attach
    /// to a newly joined world which happens to reuse the same PersonId.
    pub epoch: u64,
}

impl Key {
    pub fn new(outfit: HeroOutfit, size: u32, epoch: u64) -> Self {
        Self {
            slots: outfit.slots,
            skin: outfit.skin,
            size,
            epoch,
        }
    }
    pub fn outfit(self) -> HeroOutfit {
        HeroOutfit {
            slots: self.slots,
            skin: self.skin,
        }
    }
    pub fn bytes(self) -> usize {
        (self.size * self.size * 4) as usize
    }
}

struct Entry {
    image: Handle<Image>,
    touched: u64,
}

#[derive(Resource, Default)]
pub(super) struct PortraitCache {
    pub known: HashMap<PersonId, HeroOutfit>,
    entries: HashMap<Key, Entry>,
    pub wanted: HashSet<Key>,
    pub pending: Option<(Key, Task<Vec<u8>>)>,
    pub epoch: u64,
    pub tick: u64,
    pub bytes: usize,
    pub completed: u64,
    pub discarded: u64,
}

impl PortraitCache {
    pub fn cached(&mut self, key: Key) -> Option<Handle<Image>> {
        let entry = self.entries.get_mut(&key)?;
        entry.touched = self.tick;
        Some(entry.image.clone())
    }

    pub fn peek(&self, key: Key) -> Option<Handle<Image>> {
        self.entries.get(&key).map(|entry| entry.image.clone())
    }

    pub fn has(&self, key: Key) -> bool {
        self.entries.contains_key(&key)
    }

    pub fn finish(&mut self, key: Key, pixels: Vec<u8>, images: &mut Assets<Image>) {
        if key.epoch != self.epoch || !self.wanted.contains(&key) || self.has(key) {
            self.discarded += 1;
            return;
        }
        if pixels.len() != key.bytes() {
            return;
        }
        // Never evict an image visible elsewhere. If pathological UI content
        // pins the entire budget, defer the new portrait until space is freed.
        if !self.make_room(key.bytes(), images) {
            return;
        }
        let image = images.add(Image::new(
            Extent3d {
                width: key.size,
                height: key.size,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            pixels,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        ));
        self.entries.insert(
            key,
            Entry {
                image,
                touched: self.tick,
            },
        );
        self.bytes += key.bytes();
        self.completed += 1;
    }

    pub fn make_room(&mut self, bytes: usize, images: &mut Assets<Image>) -> bool {
        while self.bytes + bytes > TEXTURE_BUDGET {
            let oldest = self
                .entries
                .iter()
                .filter(|(key, _)| !self.wanted.contains(*key))
                .min_by_key(|(_, entry)| entry.touched)
                .map(|(key, _)| *key);
            let Some(key) = oldest else { return false };
            if let Some(entry) = self.entries.remove(&key) {
                images.remove(entry.image.id());
                self.bytes -= key.bytes();
            }
        }
        true
    }

    pub fn invalidate(&mut self, images: &mut Assets<Image>) {
        for (_, entry) in self.entries.drain() {
            images.remove(entry.image.id());
        }
        self.bytes = 0;
        self.wanted.clear();
        self.epoch = self.epoch.wrapping_add(1);
        // Do not drop an active synchronous job: it cannot be cancelled while
        // rasterizing. Let it finish and discard it before starting the next.
    }

    pub fn clear_world(&mut self, images: &mut Assets<Image>) {
        self.invalidate(images);
        self.known.clear();
        self.completed = 0;
        self.discarded = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(index: u8) -> Key {
        Key::new(
            HeroOutfit {
                skin: index,
                ..default()
            },
            192,
            0,
        )
    }
    fn finish(cache: &mut PortraitCache, key: Key, images: &mut Assets<Image>) {
        cache.wanted.insert(key);
        cache.finish(key, vec![0; key.bytes()], images);
    }

    #[test]
    fn stale_and_duplicate_outputs_never_allocate_texture_assets() {
        let mut cache = PortraitCache::default();
        let mut images = Assets::<Image>::default();
        cache.finish(key(0), vec![0; key(0).bytes()], &mut images);
        assert_eq!(images.len(), 0);
        finish(&mut cache, key(0), &mut images);
        finish(&mut cache, key(0), &mut images);
        assert_eq!(images.len(), 1);
        assert_eq!(cache.bytes, key(0).bytes());
        assert_eq!(cache.discarded, 2);
    }

    #[test]
    fn texture_budget_evicts_offscreen_lru_and_protects_visible_portrait() {
        let mut cache = PortraitCache::default();
        let mut images = Assets::<Image>::default();
        let pinned = key(0);
        finish(&mut cache, pinned, &mut images);
        let limit = TEXTURE_BUDGET / pinned.bytes();
        for n in 1..limit + 5 {
            cache.wanted.retain(|k| *k == pinned);
            cache.tick += 1;
            finish(&mut cache, key(n as u8), &mut images);
        }
        assert!(cache.has(pinned));
        assert!(!cache.has(key(1)));
        assert!(cache.bytes <= TEXTURE_BUDGET);
        assert_eq!(cache.bytes, cache.entries.len() * pinned.bytes());
    }

    #[test]
    fn session_cleanup_forgets_appearance_and_rejects_old_world_worker() {
        let mut cache = PortraitCache::default();
        let mut images = Assets::<Image>::default();
        let unrelated = images.add(Image::default());
        cache.known.insert(PersonId(7), HeroOutfit::default());
        finish(&mut cache, key(0), &mut images);
        cache.clear_world(&mut images);
        assert!(cache.known.is_empty());
        assert_eq!(images.len(), 1);
        assert!(images.contains(unrelated.id()));
        // Reusing an id/outfit after reconnect must not revive the old output.
        cache.wanted.insert(key(0));
        cache.finish(key(0), vec![0; key(0).bytes()], &mut images);
        assert_eq!(images.len(), 1);
    }

    #[test]
    fn appearance_and_resolution_are_part_of_cache_identity() {
        let original = HeroOutfit::default();
        let mut changed = original;
        changed.slots[0] = 1;
        assert_ne!(Key::new(original, 192, 0), Key::new(changed, 192, 0));
        assert_ne!(Key::new(original, 192, 0), Key::new(original, 384, 0));
        assert_ne!(Key::new(original, 192, 0), Key::new(original, 192, 1));
    }
    #[test]
    fn a_fully_pinned_budget_defers_new_textures_without_exceeding_the_limit() {
        let mut cache = PortraitCache::default();
        let mut images = Assets::<Image>::default();
        let count = TEXTURE_BUDGET / key(0).bytes();
        for n in 0..=count {
            finish(&mut cache, key(n as u8), &mut images);
        }
        assert_eq!(images.len(), count);
        assert!(!cache.has(key(count as u8)));
        assert!(cache.bytes <= TEXTURE_BUDGET);
        cache.wanted.remove(&key(0));
        finish(&mut cache, key(count as u8), &mut images);
        assert!(cache.has(key(count as u8)));
        assert!(!cache.has(key(0)));
    }
}
