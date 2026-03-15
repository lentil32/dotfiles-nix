mod cursor;
mod machine;
mod options_patch;

pub(crate) use cursor::{CursorLocation, CursorShape};
#[cfg(test)]
pub(crate) use machine::JumpCuePhase;
pub(crate) use machine::RuntimeState;
pub(crate) use options_patch::{
    ColorOptionsPatch, CtermCursorColorsPatch, MotionOptionsPatch, OptionalChange,
    ParticleOptionsPatch, RenderingOptionsPatch, RuntimeOptionsEffects, RuntimeOptionsPatch,
    RuntimeSwitchesPatch, SmearBehaviorPatch,
};
