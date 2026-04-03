use super::*;
use proptest::collection::vec;

proptest! {
    #![proptest_config(stateful_config())]

    #[test]
    fn prop_retarget_epoch_follows_operation_sequences(
        operations in vec(lifecycle_sequence_operation_strategy(), 1..24)
    ) {
        let mut state = RuntimeState::default();

        for operation in operations {
            let expected_epoch = expected_retarget_epoch(&state, &operation);

            apply_lifecycle_sequence_operation(&mut state, &operation);

            prop_assert_eq!(
                state.retarget_epoch(),
                expected_epoch,
                "operation={:?}",
                operation
            );
        }
    }
}
