pub(crate) fn simulate_step(mut input: StepInput) -> StepOutput {
    let (
        current_corners,
        velocity_corners,
        spring_velocity_corners,
        trail_elapsed_ms,
        index_head,
        index_tail,
    ) = update_corners(&input);

    let mut rng = Rng32::from_seed(input.rng_state);
    let particles = std::mem::take(&mut input.particles);
    let particle_step = update_particles(
        &input,
        &current_corners,
        &velocity_corners,
        particles,
        &mut rng,
    );

    StepOutput {
        current_corners,
        velocity_corners,
        spring_velocity_corners,
        trail_elapsed_ms,
        particles: particle_step.particles,
        previous_center: particle_step.previous_center,
        index_head,
        index_tail,
        rng_state: rng.state(),
    }
}
