use super::super::ingress::CursorAutocmdIngress;

pub(crate) fn should_request_observation_for_autocmd(ingress: CursorAutocmdIngress) -> bool {
    ingress.requests_observation_base()
}
