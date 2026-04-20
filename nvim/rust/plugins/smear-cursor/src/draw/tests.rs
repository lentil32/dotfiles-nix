use super::render_plan;
use super::test_support::base_frame;
use crate::core::types::StrokeId;

#[test]
fn trail_stroke_change_changes_draw_signature() {
    let mut baseline = base_frame();
    baseline.retarget_epoch = 10;
    let baseline_signature = render_plan::frame_draw_signature(&baseline);

    let mut retargeted = baseline;
    retargeted.trail_stroke_id = StrokeId::new(2);
    let retargeted_signature = render_plan::frame_draw_signature(&retargeted);

    assert_ne!(baseline_signature, retargeted_signature);
}
