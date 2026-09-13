//! Decode once off the main/audio threads. This intentionally small cache is for
//! short world effects, not long music/ambience. At most 8 × 2 MiB of mono PCM.
use super::*;
use bevy::audio::Source;
use bevy::tasks::{block_on, poll_once, AsyncComputeTaskPool, Task};
use std::{collections::HashMap, io::Cursor};

const MAX_CLIPS: usize = 8;
const MAX_SAMPLES: usize = 2 * 1024 * 1024 / size_of::<f32>();

enum Prepared {
    Loading(Task<Result<Arc<MonoClip>, String>>),
    Ready(Arc<MonoClip>),
    Failed,
}

#[derive(Resource, Default)]
pub(crate) struct WorldSoundCache {
    entries: HashMap<AssetId<AudioSource>, Prepared>,
}

impl WorldSoundCache {
    pub(in crate::audio) fn prepare(
        &mut self,
        handle: &Handle<AudioSource>,
        assets: &Assets<AudioSource>,
    ) -> Option<Arc<MonoClip>> {
        if !self.entries.contains_key(&handle.id()) {
            let source = assets.get(handle)?.clone();
            if self.entries.len() >= MAX_CLIPS {
                // Active sounds retain their PCM. Only an unused completed entry
                // may be evicted; loading tasks also count against the budget.
                let unused = self.entries.iter().find_map(|(key, value)| match value {
                    Prepared::Ready(clip) if Arc::strong_count(clip) == 1 => Some(*key),
                    Prepared::Failed => Some(*key),
                    _ => None,
                })?;
                self.entries.remove(&unused);
            }
            self.entries.insert(
                handle.id(),
                Prepared::Loading(
                    AsyncComputeTaskPool::get().spawn(async move { decode(source).map(Arc::new) }),
                ),
            );
        }
        let entry = self.entries.get_mut(&handle.id())?;
        if let Prepared::Loading(task) = entry {
            if let Some(result) = block_on(poll_once(task)) {
                *entry = match result {
                    Ok(clip) => Prepared::Ready(clip),
                    Err(error) => {
                        warn!("Cannot prepare world sound: {error}");
                        Prepared::Failed
                    }
                };
            }
        }
        match entry {
            Prepared::Ready(clip) => Some(clip.clone()),
            _ => None,
        }
    }

    pub(crate) fn ready_bytes(&self) -> usize {
        self.entries
            .values()
            .map(|value| match value {
                Prepared::Ready(clip) => clip.bytes(),
                _ => 0,
            })
            .sum()
    }
}

pub(super) fn decode(source: AudioSource) -> Result<MonoClip, String> {
    let mut decoder =
        rodio::Decoder::new(Cursor::new(source)).map_err(|error| error.to_string())?;
    let rate = decoder.sample_rate();
    if decoder.channels().get() != 1 {
        return Err("World effects must be exported as mono".into());
    }
    if !(8000..=48000).contains(&rate.get()) {
        return Err("World effects require an 8–48 kHz sample rate".into());
    }
    let mut samples = Vec::new();
    for sample in &mut decoder {
        if samples.len() == MAX_SAMPLES {
            return Err("World effect exceeds the 2 MiB PCM budget".into());
        }
        if !sample.is_finite() {
            return Err("World effect contains nonfinite samples".into());
        }
        samples.push(sample);
    }
    if samples.is_empty() {
        return Err("World effect has no samples".into());
    }
    Ok(MonoClip {
        samples: samples.into(),
        rate,
    })
}
