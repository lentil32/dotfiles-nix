pub(crate) const BASE_TIME_INTERVAL: f64 = 17.0;
pub(crate) const EPSILON: f64 = 1.0e-9;
pub(crate) const DEFAULT_RNG_STATE: u32 = 0xA341_316C;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Point {
    pub(crate) row: f64,
    pub(crate) col: f64,
}

impl Point {
    pub(crate) const ZERO: Self = Self { row: 0.0, col: 0.0 };

    pub(crate) fn distance_squared(self, other: Self) -> f64 {
        let dy = self.row - other.row;
        let dx = self.col - other.col;
        dy * dy + dx * dx
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Particle {
    pub(crate) position: Point,
    pub(crate) velocity: Point,
    pub(crate) lifetime: f64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct GradientInfo {
    pub(crate) origin: Point,
    pub(crate) direction_scaled: Point,
}

#[derive(Clone, Debug)]
pub(crate) struct RenderFrame {
    pub(crate) mode: String,
    pub(crate) corners: [Point; 4],
    pub(crate) target: Point,
    pub(crate) target_corners: [Point; 4],
    pub(crate) vertical_bar: bool,
    pub(crate) particles: Vec<Particle>,
    pub(crate) cursor_color: Option<String>,
    pub(crate) cursor_color_insert_mode: Option<String>,
    pub(crate) normal_bg: Option<String>,
    pub(crate) transparent_bg_fallback_color: String,
    pub(crate) cterm_cursor_colors: Option<Vec<u16>>,
    pub(crate) cterm_bg: Option<u16>,
    pub(crate) color_at_cursor: Option<String>,
    pub(crate) hide_target_hack: bool,
    pub(crate) max_kept_windows: usize,
    pub(crate) never_draw_over_target: bool,
    pub(crate) use_diagonal_blocks: bool,
    pub(crate) max_slope_horizontal: f64,
    pub(crate) min_slope_vertical: f64,
    pub(crate) max_angle_difference_diagonal: f64,
    pub(crate) max_offset_diagonal: f64,
    pub(crate) min_shade_no_diagonal: f64,
    pub(crate) min_shade_no_diagonal_vertical_bar: f64,
    pub(crate) max_shade_no_matrix: f64,
    pub(crate) particle_max_lifetime: f64,
    pub(crate) particles_over_text: bool,
    pub(crate) color_levels: u32,
    pub(crate) gamma: f64,
    pub(crate) gradient_exponent: f64,
    pub(crate) matrix_pixel_threshold: f64,
    pub(crate) matrix_pixel_threshold_vertical_bar: f64,
    pub(crate) matrix_pixel_min_factor: f64,
    pub(crate) windows_zindex: u32,
    pub(crate) gradient: Option<GradientInfo>,
}

#[derive(Debug)]
pub(crate) struct StepInput {
    pub(crate) mode: String,
    pub(crate) time_interval: f64,
    pub(crate) config_time_interval: f64,
    pub(crate) current_corners: [Point; 4],
    pub(crate) target_corners: [Point; 4],
    pub(crate) velocity_corners: [Point; 4],
    pub(crate) stiffnesses: [f64; 4],
    pub(crate) max_length: f64,
    pub(crate) max_length_insert_mode: f64,
    pub(crate) damping: f64,
    pub(crate) damping_insert_mode: f64,
    pub(crate) delay_disable: Option<f64>,
    pub(crate) particles: Vec<Particle>,
    pub(crate) previous_center: Point,
    pub(crate) particle_damping: f64,
    pub(crate) particles_enabled: bool,
    pub(crate) particle_gravity: f64,
    pub(crate) particle_random_velocity: f64,
    pub(crate) particle_max_num: usize,
    pub(crate) particle_spread: f64,
    pub(crate) particles_per_second: f64,
    pub(crate) particles_per_length: f64,
    pub(crate) particle_max_initial_velocity: f64,
    pub(crate) particle_velocity_from_cursor: f64,
    pub(crate) particle_max_lifetime: f64,
    pub(crate) particle_lifetime_distribution_exponent: f64,
    pub(crate) min_distance_emit_particles: f64,
    pub(crate) vertical_bar: bool,
    pub(crate) horizontal_bar: bool,
    pub(crate) block_aspect_ratio: f64,
    pub(crate) rng_state: u32,
}

#[derive(Debug)]
pub(crate) struct StepOutput {
    pub(crate) current_corners: [Point; 4],
    pub(crate) velocity_corners: [Point; 4],
    pub(crate) particles: Vec<Particle>,
    pub(crate) previous_center: Point,
    pub(crate) index_head: usize,
    pub(crate) index_tail: usize,
    pub(crate) disabled_due_to_delay: bool,
    pub(crate) rng_state: u32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Rng32 {
    state: u32,
}

impl Rng32 {
    pub(crate) fn from_seed(seed: u32) -> Self {
        let normalized = if seed == 0 { DEFAULT_RNG_STATE } else { seed };
        Self { state: normalized }
    }

    pub(crate) fn next_u32(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        if x == 0 {
            x = DEFAULT_RNG_STATE;
        }
        self.state = x;
        x
    }

    pub(crate) fn next_unit(&mut self) -> f64 {
        f64::from(self.next_u32()) / f64::from(u32::MAX)
    }

    pub(crate) fn state(self) -> u32 {
        self.state
    }
}
