use super::*;
use pretty_assertions::assert_eq;

#[test]
fn semantic_classifier_detects_text_mutation_cases() {
    struct TestCase {
        previous_position: CursorPosition,
        previous_line: i64,
        previous_rows: &'static [&'static str],
        current_position: CursorPosition,
        current_line: i64,
        current_rows: &'static [&'static str],
        current_tracked_rows: Option<&'static [&'static str]>,
    }

    let cases = [
        TestCase {
            previous_position: CursorPosition {
                row: CursorRow(4),
                col: CursorCol(5),
            },
            previous_line: 7,
            previous_rows: &["before", "alpha", "after"],
            current_position: CursorPosition {
                row: CursorRow(4),
                col: CursorCol(6),
            },
            current_line: 8,
            current_rows: &["alph", "a", "after"],
            current_tracked_rows: Some(&["before", "alph", "a"]),
        },
        TestCase {
            previous_position: CursorPosition {
                row: CursorRow(5),
                col: CursorCol(5),
            },
            previous_line: 9,
            previous_rows: &["header", "alpha", "tail"],
            current_position: CursorPosition {
                row: CursorRow(5),
                col: CursorCol(6),
            },
            current_line: 9,
            current_rows: &["header", "alphax", "tail"],
            current_tracked_rows: Some(&["header", "alphax", "tail"]),
        },
        TestCase {
            previous_position: CursorPosition {
                row: CursorRow(5),
                col: CursorCol(6),
            },
            previous_line: 9,
            previous_rows: &["header", "alphax", "tail"],
            current_position: CursorPosition {
                row: CursorRow(5),
                col: CursorCol(5),
            },
            current_line: 9,
            current_rows: &["header", "alpha", "tail"],
            current_tracked_rows: Some(&["header", "alpha", "tail"]),
        },
        TestCase {
            previous_position: CursorPosition {
                row: CursorRow(5),
                col: CursorCol(5),
            },
            previous_line: 9,
            previous_rows: &["header", "alpha", "tail"],
            current_position: CursorPosition {
                row: CursorRow(5),
                col: CursorCol(5),
            },
            current_line: 9,
            current_rows: &["header", "alha", "tail"],
            current_tracked_rows: Some(&["header", "alha", "tail"]),
        },
        TestCase {
            previous_position: CursorPosition {
                row: CursorRow(5),
                col: CursorCol(5),
            },
            previous_line: 9,
            previous_rows: &["header", "alpha", "tail"],
            current_position: CursorPosition {
                row: CursorRow(6),
                col: CursorCol(3),
            },
            current_line: 10,
            current_rows: &["alpha pasted", "block", "tail"],
            current_tracked_rows: Some(&["header", "alpha pasted", "block"]),
        },
        TestCase {
            previous_position: CursorPosition {
                row: CursorRow(5),
                col: CursorCol(5),
            },
            previous_line: 9,
            previous_rows: &["header", "ka", "tail"],
            current_position: CursorPosition {
                row: CursorRow(5),
                col: CursorCol(7),
            },
            current_line: 9,
            current_rows: &["header", "kana", "tail"],
            current_tracked_rows: Some(&["header", "kana", "tail"]),
        },
        TestCase {
            previous_position: CursorPosition {
                row: CursorRow(5),
                col: CursorCol(5),
            },
            previous_line: 9,
            previous_rows: &["header", "alpha", "tail"],
            current_position: CursorPosition {
                row: CursorRow(10),
                col: CursorCol(3),
            },
            current_line: 14,
            current_rows: &["inserted two", "inserted three", "tail"],
            current_tracked_rows: Some(&["header", "alpha pasted", "inserted one"]),
        },
    ];

    for case in cases {
        assert_text_mutation_classification(
            case.previous_position,
            case.previous_line,
            case.previous_rows,
            case.current_position,
            case.current_line,
            case.current_rows,
            case.current_tracked_rows,
        );
    }
}

#[test]
fn semantic_classifier_detects_motion_without_text_mutation() {
    let request = observation_request(ProbeRequestSet::default());
    let viewport = ViewportSnapshot::new(CursorRow(40), CursorCol(120));
    let previous = ObservationSnapshot::new(
        request.clone(),
        observation_basis(viewport).with_cursor_text_context_state(
            CursorTextContextState::Sampled(text_context(
                8,
                7,
                &["before", "alpha", "after"],
                None,
            )),
        ),
        ObservationMotion::default(),
    );
    let current = ObservationSnapshot::new(
        request,
        ObservationBasis::new(
            Millis::new(11),
            "n".to_string(),
            Some(CursorPosition {
                row: CursorRow(5),
                col: CursorCol(5),
            }),
            CursorLocation::new(1, 1, 1, 8),
            viewport,
        )
        .with_cursor_text_context_state(CursorTextContextState::Sampled(text_context(
            8,
            8,
            &["alpha", "after", "tail"],
            Some(&["before", "alpha", "after"]),
        ))),
        ObservationMotion::default(),
    );

    assert_eq!(
        classify_semantic_event(Some(&previous), &current),
        SemanticEvent::CursorMovedWithoutTextMutation
    );
}

#[test]
fn semantic_classifier_detects_viewport_or_window_motion_cases() {
    let request = observation_request(ProbeRequestSet::default());
    let viewport = ViewportSnapshot::new(CursorRow(40), CursorCol(120));
    let previous = ObservationSnapshot::new(
        request.clone(),
        observation_basis(viewport),
        ObservationMotion::default(),
    );
    let cases = [
        CursorLocation::new(2, 1, 3, 1),
        CursorLocation::new(1, 1, 1, 1).with_viewport_columns(3, 0),
        CursorLocation::new(1, 1, 1, 1).with_window_origin(3, 4),
        CursorLocation::new(1, 1, 1, 1).with_window_dimensions(80, 24),
    ];

    for current_location in cases {
        let current = ObservationSnapshot::new(
            request.clone(),
            ObservationBasis::new(
                Millis::new(11),
                "n".to_string(),
                Some(CursorPosition {
                    row: CursorRow(4),
                    col: CursorCol(5),
                }),
                current_location,
                viewport,
            ),
            ObservationMotion::default(),
        );

        assert_eq!(
            classify_semantic_event(Some(&previous), &current),
            SemanticEvent::ViewportOrWindowMoved
        );
    }
}

#[test]
fn semantic_classifier_detects_mode_change() {
    let previous_request = observation_request(ProbeRequestSet::default());
    let current_request = PendingObservation::new(
        ExternalDemand::new(
            IngressSeq::new(1),
            ExternalDemandKind::ModeChanged,
            Millis::new(10),
            None,
            BufferPerfClass::Full,
        ),
        ProbeRequestSet::default(),
    );
    let viewport = ViewportSnapshot::new(CursorRow(40), CursorCol(120));
    let previous = ObservationSnapshot::new(
        previous_request,
        observation_basis(viewport),
        ObservationMotion::default(),
    );
    let current = ObservationSnapshot::new(
        current_request,
        ObservationBasis::new(
            Millis::new(11),
            "i".to_string(),
            Some(CursorPosition {
                row: CursorRow(4),
                col: CursorCol(5),
            }),
            CursorLocation::new(1, 1, 1, 1),
            viewport,
        ),
        ObservationMotion::default(),
    );

    assert_eq!(
        classify_semantic_event(Some(&previous), &current),
        SemanticEvent::ModeChanged
    );
}

#[test]
fn semantic_classifier_prioritizes_text_mutation_before_viewport_motion() {
    let request = observation_request(ProbeRequestSet::default());
    let viewport = ViewportSnapshot::new(CursorRow(40), CursorCol(120));
    let previous =
        ObservationSnapshot::new(
            request.clone(),
            ObservationBasis::new(
                Millis::new(10),
                "n".to_string(),
                Some(CursorPosition {
                    row: CursorRow(5),
                    col: CursorCol(5),
                }),
                CursorLocation::new(1, 1, 1, 9),
                viewport,
            )
            .with_cursor_text_context_state(CursorTextContextState::Sampled(
                text_context(10, 9, &["header", "alpha", "tail"], None),
            )),
            ObservationMotion::default(),
        );
    let current = ObservationSnapshot::new(
        request,
        ObservationBasis::new(
            Millis::new(11),
            "n".to_string(),
            Some(CursorPosition {
                row: CursorRow(6),
                col: CursorCol(3),
            }),
            CursorLocation::new(1, 1, 4, 10),
            viewport,
        )
        .with_cursor_text_context_state(CursorTextContextState::Sampled(text_context(
            11,
            10,
            &["alpha pasted", "block", "tail"],
            Some(&["header", "alpha pasted", "block"]),
        ))),
        ObservationMotion::default(),
    );

    assert_eq!(
        classify_semantic_event(Some(&previous), &current),
        SemanticEvent::TextMutatedAtCursorContext
    );
}
