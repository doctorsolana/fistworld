//! Async profile write queue to keep disk IO off fixed-tick hot paths.

use bevy::prelude::*;
use shared::player_profile::PlayerProfile;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SavePriority {
    Normal,
    High,
}

#[derive(Clone, Debug)]
struct SaveJob {
    id: u64,
    profile: PlayerProfile,
    priority: SavePriority,
}

#[derive(Clone, Debug)]
pub struct SaveJobAck {
    pub id: u64,
    pub player_name: String,
    pub priority: SavePriority,
    pub result: Result<(), String>,
}

#[derive(Default)]
struct SaveQueueState {
    jobs: VecDeque<SaveJob>,
}

/// Worker-backed profile save queue with high-priority disconnect submissions.
#[derive(Resource)]
pub struct ProfileIoQueue {
    queue: Arc<(Mutex<SaveQueueState>, Condvar)>,
    acks: Arc<Mutex<Vec<SaveJobAck>>>,
    next_job_id: u64,
    pending_disconnect_jobs: HashMap<u64, String>,
}

impl ProfileIoQueue {
    pub fn new(storage_dir: PathBuf) -> Self {
        let queue = Arc::new((Mutex::new(SaveQueueState::default()), Condvar::new()));
        let acks = Arc::new(Mutex::new(Vec::new()));

        let worker_queue = Arc::clone(&queue);
        let worker_acks = Arc::clone(&acks);

        std::thread::Builder::new()
            .name("profile-io-worker".to_string())
            .spawn(move || loop {
                let job = {
                    let (lock, cvar) = &*worker_queue;
                    let mut state = lock
                        .lock()
                        .expect("profile io queue lock poisoned while waiting for work");
                    while state.jobs.is_empty() {
                        state = cvar
                            .wait(state)
                            .expect("profile io queue lock poisoned while waiting");
                    }
                    state.jobs.pop_front()
                };

                let Some(job) = job else { continue };

                let player_name = job.profile.player_name.clone();
                let result =
                    crate::persistence::profiles::save_profile_to_dir(&storage_dir, &job.profile);
                let ack = SaveJobAck {
                    id: job.id,
                    player_name,
                    priority: job.priority,
                    result,
                };

                if let Ok(mut ack_queue) = worker_acks.lock() {
                    ack_queue.push(ack);
                }
            })
            .expect("failed to spawn profile io worker thread");

        Self {
            queue,
            acks,
            next_job_id: 1,
            pending_disconnect_jobs: HashMap::new(),
        }
    }

    pub fn enqueue_profile_save(&mut self, profile: PlayerProfile, priority: SavePriority) -> u64 {
        let id = self.next_job_id;
        self.next_job_id = self.next_job_id.wrapping_add(1);

        let job = SaveJob {
            id,
            profile,
            priority,
        };

        let (lock, cvar) = &*self.queue;
        if let Ok(mut state) = lock.lock() {
            match priority {
                SavePriority::High => state.jobs.push_front(job),
                SavePriority::Normal => state.jobs.push_back(job),
            }
            cvar.notify_one();
        }

        id
    }

    pub fn track_disconnect_job(&mut self, job_id: u64, player_name: String) {
        self.pending_disconnect_jobs.insert(job_id, player_name);
    }

    pub fn drain_acks(&mut self) -> Vec<SaveJobAck> {
        let Ok(mut ack_queue) = self.acks.lock() else {
            return Vec::new();
        };

        let mut drained = Vec::new();
        std::mem::swap(&mut *ack_queue, &mut drained);
        drained
    }
}

/// Poll and log save acknowledgements from the background worker.
pub fn update_profile_io_acks(mut io_queue: ResMut<ProfileIoQueue>) {
    for ack in io_queue.drain_acks() {
        let was_disconnect = io_queue.pending_disconnect_jobs.remove(&ack.id).is_some();
        match ack.result {
            Ok(()) => {
                if was_disconnect {
                    info!(
                        "Disconnect profile save completed for '{}' (job {})",
                        ack.player_name, ack.id
                    );
                }
            }
            Err(error) => {
                error!(
                    "Profile save failed for '{}' (job {}, priority {:?}): {}",
                    ack.player_name, ack.id, ack.priority, error
                );
            }
        }
    }
}
