//! Read-only evidence for the connected audio rehearsal; never installed on its own.
use crate::audio::{
    carts::{CartAudioMetrics, CartRollVoice},
    music::MusicCue,
    sfx::{SfxCue, SfxStats, UiSoundVoice},
    AudioSettings,
};
use bevy::prelude::*;
use serde_json::{json, Value};
use shared::{
    components::{CharacterActivity, CharacterMotion, PersonId},
    economy::PorterCartState,
};

pub(super) fn snapshot(world: &mut World) -> Value {
    let filtered_pcm_bytes = world
        .get_resource::<crate::audio::filtered::WorldSoundCache>()
        .map(|cache| cache.ready_bytes());
    let filtered_sources = world
        .get_resource::<Assets<crate::audio::filtered::FilteredSound>>()
        .map(|assets| assets.len());
    let settings = world.get_resource::<AudioSettings>().map(|s| json!(s));
    let stats = world.get_resource::<SfxStats>().map(|s| json!({
        "started": SfxCue::ALL.into_iter().map(|cue| (format!("{cue:?}"),json!(s.started[cue as usize]))).collect::<serde_json::Map<_,_>>(),
        "coalesced":s.coalesced,"unavailable":s.unavailable,"muted":s.muted,
        "budget_dropped":s.budget_dropped,"expired_pending":s.expired_pending}));
    let carts = world.get_resource::<CartAudioMetrics>().map(|s| json!({
        "candidates":s.candidates,"active":s.active,"pending":s.pending,"starts":s.starts,"stops":s.stops,"timed_out":s.timed_out}));
    let ui:Vec<_> = world.query::<(&UiSoundVoice,Option<&AudioSink>)>().iter(world)
        .map(|(v,s)|json!({"cue":format!("{:?}",v.cue),"started_at":v.started_at,"sink":s.is_some(),"position":s.map(|s|s.position().as_secs_f64()),"volume":s.map(|s|s.volume().to_linear())})).collect();
    let voices:Vec<_> = world.query::<(Entity,&CartRollVoice,&Transform,Option<&bevy::audio::SpatialAudioSink>)>().iter(world)
        .map(|(entity,v,t,s)|json!({"voice_entity":format!("{entity:?}"),"owner":format!("{:?}",v.owner),"position":t.translation.to_array(),"gain":v.gain,"speed":v.speed,"cutoff_hz":v.cutoff_hz,"estimated_gain":v.estimated_gain,"sink":s.is_some(),"playback_position":s.map(|s|s.position().as_secs_f64())})).collect();
    let sources:Vec<_> = world.query_filtered::<(Entity,&PersonId,&Transform,&crate::hero::HeroVisual,Option<&CharacterMotion>,Option<&CharacterActivity>),With<PorterCartState>>().iter(world)
        .map(|(e,p,t,v,m,a)|json!({"entity":format!("{e:?}"),"id":p.0,"position":t.translation.to_array(),"speed":v.speed(),"velocity":m.map(|m|m.velocity.to_array()),"activity":format!("{a:?}")})).collect();
    let music:Vec<_> = world.query::<(Entity,&MusicCue,Option<&AudioSink>)>().iter(world)
        .map(|(e,c,s)|json!({"entity":format!("{e:?}"),"cue":format!("{c:?}"),"sink":s.is_some(),"paused":s.map(|s|s.is_paused()),"position":s.map(|s|s.position().as_secs_f64()),"volume":s.map(|s|s.volume().to_linear())})).collect();
    json!({"filtered_pcm_bytes":filtered_pcm_bytes,"filtered_sources":filtered_sources,"settings":settings,"stats":stats,"ui":ui,"carts":carts,"cart_voices":voices,"cart_sources":sources,"music":music})
}
