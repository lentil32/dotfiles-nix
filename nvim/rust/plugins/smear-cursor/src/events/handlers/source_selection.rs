use super::super::ingress::AutocmdIngress;

pub(crate) fn should_request_observation_for_autocmd(ingress: AutocmdIngress) -> bool {
    ingress.requests_observation_base()
}
