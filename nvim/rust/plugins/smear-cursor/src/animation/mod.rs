use crate::config::RuntimeConfig;
use crate::types::BASE_TIME_INTERVAL;
use crate::types::EPSILON;
use crate::types::Particle;
use crate::types::Point;
use crate::types::Rng32;
use crate::types::StepInput;
use crate::types::StepOutput;
use nvimrs_nvim_utils::mode::is_insert_like_mode;

// Animation pipeline is split by deterministic phases:
// 1) motion primitives + stop metrics
// 2) trail-profile and duration policies
// 3) corner simulation
// 4) particle simulation
// 5) step orchestration
include!("motion_primitives.rs");
include!("trail_profile.rs");
include!("corners_sim.rs");
include!("particles.rs");
include!("simulate.rs");

#[cfg(test)]
mod tests;
