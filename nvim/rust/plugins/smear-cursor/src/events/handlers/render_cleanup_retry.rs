use crate::core::event::Event as CoreEvent;
use crate::core::event::RenderCleanupRetainedResourcesObservedEvent;
use crate::core::types::Millis;

pub(in crate::events::handlers) fn retained_resource_cleanup_retry_event(
    retained_resources: usize,
    observed_at: Millis,
) -> Option<CoreEvent> {
    (retained_resources > 0).then_some(CoreEvent::RenderCleanupRetainedResourcesObserved(
        RenderCleanupRetainedResourcesObservedEvent {
            observed_at,
            retained_resources,
        },
    ))
}
