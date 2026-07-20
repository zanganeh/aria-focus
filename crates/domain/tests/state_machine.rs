use domain::{
    Activity, Intensity, Session, SessionError, SessionPhase, SessionStatus, SessionType,
    TrackEnjoyment, MAX_COUNTDOWN_SECONDS,
};

fn session(kind: SessionType) -> Session {
    Session::new(Activity::DeepWork, Intensity::Medium, kind).unwrap()
}

fn interval() -> SessionType {
    SessionType::Interval {
        work_seconds: 120,
        break_seconds: 60,
        repeats: 3,
    }
}

#[test]
fn enjoyment_has_only_explicit_user_meaningful_storage_values() {
    assert_eq!(TrackEnjoyment::Liked.storage_key(), "liked");
    assert_eq!(TrackEnjoyment::NotForMe.storage_key(), "not_for_me");
    assert_eq!(TrackEnjoyment::from_storage_key("unknown"), None);
}

#[test]
fn infinite_counts_focus_up_and_has_no_remaining_time() {
    let mut session = session(SessionType::Infinite);
    session.start(10).unwrap();
    let snapshot = session.tick(70);
    assert_eq!(snapshot.phase, Some(SessionPhase::Work));
    assert_eq!(snapshot.focus_elapsed_seconds, 60);
    assert_eq!(snapshot.current_phase_remaining_seconds, None);
    assert_eq!(snapshot.total_remaining_seconds, None);
}

#[test]
fn countdown_counts_down_and_expires_at_the_exact_boundary() {
    let mut session = session(SessionType::Countdown { seconds: 60 });
    session.start(100).unwrap();
    let before = session.tick(159);
    assert_eq!(before.status, SessionStatus::Playing);
    assert_eq!(before.focus_elapsed_seconds, 59);
    assert_eq!(before.current_phase_remaining_seconds, Some(1));
    assert_eq!(before.total_remaining_seconds, Some(1));

    let exact = session.tick(160);
    assert_eq!(exact.status, SessionStatus::Expired);
    assert_eq!(exact.phase, None);
    assert_eq!(exact.focus_elapsed_seconds, 60);
    assert_eq!(exact.total_remaining_seconds, Some(0));
    assert_eq!(session.tick(u64::MAX).focus_elapsed_seconds, 60);
}

#[test]
fn interval_exact_boundaries_alternate_and_omit_final_break() {
    let mut session = session(interval());
    session.start(0).unwrap();

    let work = session.tick(119);
    assert_eq!(work.phase, Some(SessionPhase::Work));
    assert_eq!(work.current_round, Some(1));
    assert_eq!(work.current_phase_remaining_seconds, Some(1));
    assert_eq!(work.focus_elapsed_seconds, 119);

    let rest = session.tick(120);
    assert_eq!(rest.phase, Some(SessionPhase::Break));
    assert_eq!(rest.current_round, Some(1));
    assert_eq!(rest.current_phase_remaining_seconds, Some(60));
    assert_eq!(rest.focus_elapsed_seconds, 120);

    let next_work = session.tick(180);
    assert_eq!(next_work.phase, Some(SessionPhase::Work));
    assert_eq!(next_work.current_round, Some(2));
    assert_eq!(next_work.total_rounds, Some(3));

    let final_work = session.tick(360);
    assert_eq!(final_work.phase, Some(SessionPhase::Work));
    assert_eq!(final_work.current_round, Some(3));
    assert_eq!(final_work.current_phase_remaining_seconds, Some(120));

    let expired = session.tick(480);
    assert_eq!(expired.status, SessionStatus::Expired);
    assert_eq!(expired.phase, None);
    assert_eq!(expired.focus_elapsed_seconds, 360);
    assert_eq!(expired.total_remaining_seconds, Some(0));
}

#[test]
fn large_jump_crosses_multiple_interval_boundaries_deterministically() {
    let mut session = session(interval());
    session.start(1_000).unwrap();
    let snapshot = session.tick(1_370);
    assert_eq!(snapshot.phase, Some(SessionPhase::Work));
    assert_eq!(snapshot.current_round, Some(3));
    assert_eq!(snapshot.current_phase_remaining_seconds, Some(110));
    assert_eq!(snapshot.focus_elapsed_seconds, 250);
    assert_eq!(snapshot.total_remaining_seconds, Some(110));
}

#[test]
fn pause_freezes_exactly_during_work_and_break() {
    let mut work_pause = session(interval());
    work_pause.start(0).unwrap();
    work_pause.pause(30).unwrap();
    let frozen = work_pause.snapshot(10_000);
    assert_eq!(frozen.phase, Some(SessionPhase::Work));
    assert_eq!(frozen.focus_elapsed_seconds, 30);
    assert_eq!(frozen.current_phase_remaining_seconds, Some(90));
    work_pause.resume(20_000).unwrap();
    assert_eq!(work_pause.tick(20_010).focus_elapsed_seconds, 40);

    let mut break_pause = session(interval());
    break_pause.start(0).unwrap();
    break_pause.pause(150).unwrap();
    let frozen = break_pause.snapshot(u64::MAX);
    assert_eq!(frozen.phase, Some(SessionPhase::Break));
    assert_eq!(frozen.focus_elapsed_seconds, 120);
    assert_eq!(frozen.current_phase_remaining_seconds, Some(30));
    break_pause.resume(500).unwrap();
    let next = break_pause.tick(530);
    assert_eq!(next.phase, Some(SessionPhase::Work));
    assert_eq!(next.current_round, Some(2));
}

#[test]
fn non_monotonic_input_never_fabricates_time() {
    let mut session = session(SessionType::Infinite);
    session.start(100).unwrap();
    assert_eq!(session.tick(50).focus_elapsed_seconds, 0);
    assert_eq!(session.tick(150).focus_elapsed_seconds, 50);
    assert_eq!(session.tick(120).focus_elapsed_seconds, 50);
}

#[test]
fn restart_after_stop_or_expiry_resets_timing_and_round() {
    let mut stopped = session(interval());
    stopped.start(0).unwrap();
    stopped.stop(200).unwrap();
    assert_eq!(stopped.status(), SessionStatus::Stopped);
    stopped.start(1_000).unwrap();
    let restarted = stopped.tick(1_010);
    assert_eq!(restarted.current_round, Some(1));
    assert_eq!(restarted.focus_elapsed_seconds, 10);

    let mut expired = session(SessionType::Countdown { seconds: 60 });
    expired.start(0).unwrap();
    expired.tick(60);
    expired.start(100).unwrap();
    assert_eq!(expired.tick(101).focus_elapsed_seconds, 1);
}

#[test]
fn reset_timer_restarts_the_active_track_clock_without_stopping_transport() {
    let mut playing = session(interval());
    playing.start(0).unwrap();
    assert_eq!(playing.tick(150).phase, Some(SessionPhase::Break));

    playing.reset_timer(1_000).unwrap();
    let restarted = playing.tick(1_010);
    assert_eq!(playing.status(), SessionStatus::Playing);
    assert_eq!(restarted.phase, Some(SessionPhase::Work));
    assert_eq!(restarted.current_round, Some(1));
    assert_eq!(restarted.focus_elapsed_seconds, 10);
    assert_eq!(restarted.current_phase_remaining_seconds, Some(110));

    playing.pause(1_020).unwrap();
    playing.reset_timer(2_000).unwrap();
    let paused = playing.snapshot(u64::MAX);
    assert_eq!(playing.status(), SessionStatus::Paused);
    assert_eq!(paused.focus_elapsed_seconds, 0);
    assert_eq!(paused.current_phase_remaining_seconds, Some(120));
}

#[test]
fn timer_configs_are_bounded_and_overflow_safe() {
    assert!(matches!(
        Session::new(
            Activity::DeepWork,
            Intensity::Medium,
            SessionType::Countdown { seconds: 0 }
        ),
        Err(SessionError::InvalidCountdownConfig)
    ));
    assert!(SessionType::Countdown {
        seconds: MAX_COUNTDOWN_SECONDS
    }
    .validate()
    .is_ok());
    assert!(SessionType::Countdown {
        seconds: MAX_COUNTDOWN_SECONDS + 1
    }
    .validate()
    .is_err());
    for invalid in [
        SessionType::Interval {
            work_seconds: 0,
            break_seconds: 60,
            repeats: 1,
        },
        SessionType::Interval {
            work_seconds: 60,
            break_seconds: 0,
            repeats: 1,
        },
        SessionType::Interval {
            work_seconds: 60,
            break_seconds: 60,
            repeats: 0,
        },
        SessionType::Interval {
            work_seconds: u64::MAX,
            break_seconds: u64::MAX,
            repeats: u32::MAX,
        },
    ] {
        assert!(invalid.validate().is_err());
    }
}

#[test]
fn timer_and_activity_changes_are_inactive_only() {
    let mut session = Session::default();
    session
        .set_session_type(SessionType::Countdown { seconds: 300 })
        .unwrap();
    session.select_activity(Activity::Learning).unwrap();
    session.start(0).unwrap();
    assert!(matches!(
        session.set_session_type(SessionType::Infinite),
        Err(SessionError::TimerChangeWhileActive)
    ));
    assert!(matches!(
        session.select_activity(Activity::Creativity),
        Err(SessionError::ActivityChangeWhileActive)
    ));
    session.pause(10).unwrap();
    assert!(session.set_session_type(SessionType::Infinite).is_err());
    session.stop(20).unwrap();
    session.set_session_type(SessionType::Infinite).unwrap();
}

#[test]
fn intensity_changes_do_not_reset_timer_progress() {
    let mut session = session(interval());
    session.start(0).unwrap();
    session.tick(150);
    session.set_intensity(Intensity::High);
    let snapshot = session.snapshot(150);
    assert_eq!(snapshot.phase, Some(SessionPhase::Break));
    assert_eq!(snapshot.focus_elapsed_seconds, 120);
}
