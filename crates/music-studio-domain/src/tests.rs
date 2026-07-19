use super::*;
use domain::Activity;

fn id(value: &str) -> StudioId {
    StudioId::new(value).unwrap()
}
fn request() -> StudioPromptInput {
    StudioPromptInput::new(
        Activity::DeepWork,
        id("ambient"),
        Some(id("calm")),
        StudioEnergy::Medium,
        vec![id("piano")],
        Some("slow pulse".into()),
        None,
        StudioDuration::Seconds180,
    )
    .unwrap()
}
fn job_id(value: &str) -> StudioJobId {
    StudioJobId::new(value).unwrap()
}
fn attempt_id(value: &str) -> StudioAttemptId {
    StudioAttemptId::new(value).unwrap()
}
fn record() -> StudioJobRecord {
    let request = request();
    let prompt = build_studio_prompt(&request, 42).unwrap();
    StudioJobRecord::new(
        job_id("job_abcdefghijkl"),
        attempt_id("attempt_abcdefghijkl"),
        request,
        prompt,
        10,
    )
    .unwrap()
}

#[test]
fn serde_is_strict_and_duration_is_constrained() {
    assert!(serde_json::from_str::<StudioPromptInput>(
        r#"{"activity":"deep_work","genre_id":"ambient","unknown":true}"#
    )
    .is_err());
    assert!(serde_json::from_str::<StudioPromptInput>(
        r#"{"activity":"deep_work","genre_id":"ambient","duration":120}"#
    )
    .is_err());
    let parsed: StudioPromptInput =
        serde_json::from_str(r#"{"activity":"deep_work","genre_id":"ambient"}"#).unwrap();
    assert_eq!(parsed.duration, StudioDuration::Seconds180);
    assert_eq!(
        serde_json::to_string(&StudioDuration::Seconds90).unwrap(),
        "90"
    );
    assert!(serde_json::from_str::<BuiltStudioPrompt>(r#"{"template_version":1,"creative_prompt":"instrumental music only","locked_negative_prompt":"lyrics","duration_seconds":180,"seed":1}"#).is_err());
    assert!(serde_json::from_str::<StudioFailureDetails>(&format!(
        r#"{{"code":"invalid_request","detail":"{}"}}"#,
        "x".repeat(501)
    ))
    .is_err());
    assert!(serde_json::from_str::<StudioPromptInput>(
        r#"{"activity":"voiceover","genre_id":"ambient"}"#
    )
    .is_err());
}

#[test]
fn persisted_structures_reject_unknown_fields_and_job_prompt_tampering() {
    let mut request_json = serde_json::to_value(request()).unwrap();
    request_json["unknown"] = serde_json::Value::Bool(true);
    assert!(serde_json::from_value::<StudioPromptInput>(request_json).is_err());

    let mut prompt_json =
        serde_json::to_value(build_studio_prompt(&request(), 8).unwrap()).unwrap();
    prompt_json["unknown"] = serde_json::Value::Bool(true);
    assert!(serde_json::from_value::<BuiltStudioPrompt>(prompt_json).is_err());

    assert!(serde_json::from_str::<StudioFailureDetails>(
        r#"{"code":"invalid_request","detail":"failure","unknown":true}"#
    )
    .is_err());

    let mut job_json = serde_json::to_value(record()).unwrap();
    job_json["unknown"] = serde_json::Value::Bool(true);
    assert!(serde_json::from_value::<StudioJobRecord>(job_json).is_err());

    let mut tampered_job = serde_json::to_value(record()).unwrap();
    tampered_job["prompt"]["creative_prompt"] =
        serde_json::Value::String("instrumental music only; activity: learning".into());
    assert!(serde_json::from_value::<StudioJobRecord>(tampered_job).is_err());
}
#[test]
fn ids_text_and_count_are_validated() {
    assert!(StudioId::new(" ").is_err());
    assert!(StudioId::new("UPPER").is_err());
    assert!(StudioId::new("vocal-like").is_err());
    assert!(StudioJobId::new("job_short").is_err());
    assert!(StudioAttemptId::new("attempt_abc\n123456789").is_err());
    assert!(StudioPromptInput::new(
        Activity::DeepWork,
        id("ambient"),
        None,
        StudioEnergy::Low,
        vec![id("a"), id("b"), id("c"), id("d"), id("e"), id("f")],
        None,
        None,
        StudioDuration::Seconds90
    )
    .is_err());
    assert!(StudioPromptInput::new(
        Activity::DeepWork,
        id("ambient"),
        None,
        StudioEnergy::Low,
        vec![],
        Some("x".repeat(501)),
        None,
        StudioDuration::Seconds90
    )
    .is_err());
    assert!(StudioPromptInput::new(
        Activity::DeepWork,
        id("ambient"),
        None,
        StudioEnergy::Low,
        vec![],
        Some("voiceover passage".into()),
        None,
        StudioDuration::Seconds90
    )
    .is_err());
    assert!(StudioPromptInput::new(
        Activity::DeepWork,
        id("ambient"),
        None,
        StudioEnergy::Low,
        vec![],
        None,
        Some("spokenword section".into()),
        StudioDuration::Seconds90
    )
    .is_err());
    assert!(StudioId::new("piano-roll").is_ok());
}
#[test]
fn prompt_is_deterministic_locked_and_instrumental() {
    let input = request();
    let one = build_studio_prompt(&input, 7).unwrap();
    assert_eq!(one, build_studio_prompt(&input, 7).unwrap());
    assert_eq!(one.seed, 7);
    assert_eq!(one.duration_seconds, 180);
    assert!(one.creative_prompt.contains("instrumental"));
    assert_eq!(one.locked_negative_prompt, LOCKED_NEGATIVE_PROMPT);
    assert!(StudioPromptInput::new(
        Activity::DeepWork,
        id("ambient"),
        None,
        StudioEnergy::Low,
        vec![],
        Some("vocal-like texture".into()),
        None,
        StudioDuration::Seconds90
    )
    .is_err());
    assert!(StudioPromptInput::new(
        Activity::DeepWork,
        id("ambient"),
        None,
        StudioEnergy::Low,
        vec![],
        None,
        Some("spoken words".into()),
        StudioDuration::Seconds90
    )
    .is_err());
}

#[test]
fn tempo_is_bounded_and_included_in_the_deterministic_prompt() {
    let input = StudioPromptInput::new_with_tempo(
        Activity::DeepWork,
        id("ambient"),
        Some(id("focused")),
        StudioEnergy::Medium,
        vec![id("piano")],
        Some("steady rain".into()),
        Some("gentle, even texture".into()),
        Some(90),
        StudioDuration::Seconds90,
    )
    .unwrap();
    let prompt = build_studio_prompt(&input, 42).unwrap();
    assert!(prompt.creative_prompt.contains("tempo: 90 BPM"));
    assert!(StudioPromptInput::new_with_tempo(
        Activity::DeepWork,
        id("ambient"),
        None,
        StudioEnergy::Medium,
        vec![],
        None,
        None,
        Some(201),
        StudioDuration::Seconds90,
    )
    .is_err());
}
#[test]
fn all_allowed_and_disallowed_transitions_are_enforced() {
    let cases = [
        (StudioJobState::Queued, StudioJobState::Generating),
        (StudioJobState::Queued, StudioJobState::Cancelled),
        (StudioJobState::Queued, StudioJobState::Failed),
        (StudioJobState::Queued, StudioJobState::Interrupted),
        (StudioJobState::Generating, StudioJobState::Analyzing),
        (StudioJobState::Generating, StudioJobState::Cancelled),
        (StudioJobState::Generating, StudioJobState::Failed),
        (StudioJobState::Generating, StudioJobState::Interrupted),
        (StudioJobState::Analyzing, StudioJobState::Ready),
        (StudioJobState::Analyzing, StudioJobState::Rejected),
        (StudioJobState::Analyzing, StudioJobState::Failed),
        (StudioJobState::Analyzing, StudioJobState::Interrupted),
        (StudioJobState::Ready, StudioJobState::Saving),
        (StudioJobState::Ready, StudioJobState::Cancelled),
        (StudioJobState::Saving, StudioJobState::Saved),
        (StudioJobState::Saving, StudioJobState::Ready),
        (StudioJobState::Saving, StudioJobState::Failed),
    ];
    for (from, to) in cases {
        assert!(from.allows(to));
    }
    for state in [
        StudioJobState::Queued,
        StudioJobState::Generating,
        StudioJobState::Analyzing,
        StudioJobState::Ready,
        StudioJobState::Rejected,
        StudioJobState::Failed,
        StudioJobState::Cancelled,
        StudioJobState::Interrupted,
        StudioJobState::Saving,
        StudioJobState::Saved,
    ] {
        for next in [
            StudioJobState::Queued,
            StudioJobState::Generating,
            StudioJobState::Analyzing,
            StudioJobState::Ready,
            StudioJobState::Rejected,
            StudioJobState::Failed,
            StudioJobState::Cancelled,
            StudioJobState::Interrupted,
            StudioJobState::Saving,
            StudioJobState::Saved,
        ] {
            if !cases.contains(&(state, next)) {
                assert!(!state.allows(next));
            }
        }
    }

    let mut job = record();
    assert_eq!(
        job.transition(0, StudioJobState::Ready, 11, None)
            .unwrap_err()
            .code,
        StudioErrorCode::InvalidTransition
    );
    job.transition(0, StudioJobState::Generating, 11, None)
        .unwrap();
    assert_eq!(
        job.transition(1, StudioJobState::Saved, 12, None)
            .unwrap_err()
            .code,
        StudioErrorCode::InvalidTransition
    );
}
#[test]
fn revisions_terminal_states_and_failures_are_bounded() {
    let mut job = record();
    job.transition(0, StudioJobState::Generating, 11, None)
        .unwrap();
    assert_eq!(
        job.transition(0, StudioJobState::Analyzing, 12, None)
            .unwrap_err()
            .code,
        StudioErrorCode::StaleRevision
    );
    job.transition(
        1,
        StudioJobState::Failed,
        12,
        Some(StudioFailureDetails::new(StudioErrorCode::InvalidRequest, "failed".into()).unwrap()),
    )
    .unwrap();
    assert!(job
        .transition(2, StudioJobState::Generating, 13, None)
        .is_err());
    assert!(StudioFailureDetails::new(StudioErrorCode::InvalidRequest, "x".repeat(501)).is_err());
}
#[test]
fn retry_creates_a_new_job_and_attempt() {
    let mut old = record();
    old.transition(0, StudioJobState::Generating, 11, None)
        .unwrap();
    old.transition(1, StudioJobState::Cancelled, 12, None)
        .unwrap();
    let retry = old
        .retry_as(
            job_id("job_abcdefghijkm"),
            attempt_id("attempt_abcdefghijkm"),
            13,
        )
        .unwrap();
    assert_eq!(retry.state, StudioJobState::Queued);
    assert_eq!(retry.revision, 0);
    assert_ne!(retry.job_id, old.job_id);
    assert_ne!(retry.attempt_id, old.attempt_id);
}
