//! Domain core for the ADHD Music focus player.
//!
//! Implements the session state machine, stimulation intensity, the five Focus
//! activities, and reserved future mental states. This crate has no audio or
//! persistence dependencies: domain selection and DSP/storage adapters remain
//! independent, as required by the architecture.

use serde::{Deserialize, Serialize};

/// Mental state axis. Version 1 supports `Focus`; the other variants are
/// reserved so future states can be added without changing stored records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MentalState {
    #[default]
    Focus,
    /// Reserved for a later release.
    Relax,
    /// Reserved for a later release.
    Sleep,
    /// Reserved for a later release.
    Meditate,
}

/// Focus activities used by the Phase 1B1 selector and local preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    #[default]
    DeepWork,
    Motivation,
    Creativity,
    Learning,
    LightWork,
}

impl Activity {
    pub fn label(self) -> &'static str {
        match self {
            Activity::DeepWork => "Deep Work",
            Activity::Motivation => "Motivation",
            Activity::Creativity => "Creativity",
            Activity::Learning => "Learning",
            Activity::LightWork => "Light Work",
        }
    }

    /// Short non-medical description used in the UI.
    pub fn description(self) -> &'static str {
        match self {
            Activity::DeepWork => "Sustained, cognitively demanding work.",
            Activity::Motivation => "Starting avoided or low-reward tasks.",
            Activity::Creativity => "Open-ended writing, design, and ideation.",
            Activity::Learning => "Reading, comprehension, and retention.",
            Activity::LightWork => "Email, filing, and repetitive administration.",
        }
    }

    pub fn storage_key(self) -> &'static str {
        match self {
            Activity::DeepWork => "deep_work",
            Activity::Motivation => "motivation",
            Activity::Creativity => "creativity",
            Activity::Learning => "learning",
            Activity::LightWork => "light_work",
        }
    }

    pub fn from_storage_key(value: &str) -> Option<Self> {
        match value {
            "deep_work" => Some(Activity::DeepWork),
            "motivation" => Some(Activity::Motivation),
            "creativity" => Some(Activity::Creativity),
            "learning" => Some(Activity::Learning),
            "light_work" => Some(Activity::LightWork),
            _ => None,
        }
    }
}

/// A deliberately small, explicit per-track preference for one activity.
/// This is a user preference, not a clinical assessment or treatment signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackFeedback {
    HelpsFocus,
    Neutral,
    Distracting,
}

/// Whether the listener enjoyed an installed track for one activity. This is
/// intentionally independent from focus effectiveness and is never used by
/// playback selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackEnjoyment {
    Liked,
    NotForMe,
}

impl TrackEnjoyment {
    pub fn storage_key(self) -> &'static str {
        match self {
            Self::Liked => "liked",
            Self::NotForMe => "not_for_me",
        }
    }

    pub fn from_storage_key(value: &str) -> Option<Self> {
        match value {
            "liked" => Some(Self::Liked),
            "not_for_me" => Some(Self::NotForMe),
            _ => None,
        }
    }
}

impl TrackFeedback {
    pub fn storage_key(self) -> &'static str {
        match self {
            Self::HelpsFocus => "helps_focus",
            Self::Neutral => "neutral",
            Self::Distracting => "distracting",
        }
    }

    pub fn from_storage_key(value: &str) -> Option<Self> {
        match value {
            "helps_focus" => Some(Self::HelpsFocus),
            "neutral" => Some(Self::Neutral),
            "distracting" => Some(Self::Distracting),
            _ => None,
        }
    }
}

/// Stimulation intensity. Controls DSP parameters, never master volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Intensity {
    Off = 0,
    Low = 1,
    #[default]
    Medium = 2,
    High = 3,
}

impl Intensity {
    pub fn label(self) -> &'static str {
        match self {
            Intensity::Off => "Off",
            Intensity::Low => "Low",
            Intensity::Medium => "Medium",
            Intensity::High => "High / ADHD",
        }
    }

    /// Accessible, non-colour description. The UI must not rely on colour alone.
    pub fn aria(self) -> &'static str {
        match self {
            Intensity::Off => "No stimulation processing",
            Intensity::Low => "Subtle stimulation, level 1 of 3",
            Intensity::Medium => "Default functional profile, level 2 of 3",
            Intensity::High => "Strongest profile, level 3 of 3, opt-in",
        }
    }

    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Intensity::Off),
            1 => Some(Intensity::Low),
            2 => Some(Intensity::Medium),
            3 => Some(Intensity::High),
            _ => None,
        }
    }

    pub fn storage_key(self) -> &'static str {
        match self {
            Intensity::Off => "off",
            Intensity::Low => "low",
            Intensity::Medium => "medium",
            Intensity::High => "high",
        }
    }

    pub fn from_storage_key(value: &str) -> Option<Self> {
        match value {
            "off" => Some(Intensity::Off),
            "low" => Some(Intensity::Low),
            "medium" => Some(Intensity::Medium),
            "high" => Some(Intensity::High),
            _ => None,
        }
    }
}

/// Validated global master-output level. This is intentionally separate from
/// stimulation intensity: it only controls post-DSP output gain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MasterVolume(u8);

impl MasterVolume {
    pub const DEFAULT: Self = Self(70);
    pub const MIN: u8 = 0;
    pub const MAX: u8 = 100;

    pub fn new(percent: u8) -> Result<Self, MasterVolumeError> {
        if percent <= Self::MAX {
            Ok(Self(percent))
        } else {
            Err(MasterVolumeError::OutOfRange(percent))
        }
    }

    pub fn percent(self) -> u8 {
        self.0
    }
    pub fn linear_gain(self) -> f32 {
        f32::from(self.0) / f32::from(Self::MAX)
    }
}

impl Default for MasterVolume {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MasterVolumeError {
    #[error("master volume must be between 0 and 100 percent, got {0}")]
    OutOfRange(u8),
}

pub const MIN_COUNTDOWN_SECONDS: u64 = 60;
pub const MAX_COUNTDOWN_SECONDS: u64 = 8 * 60 * 60;
pub const MIN_INTERVAL_WORK_SECONDS: u64 = 60;
pub const MAX_INTERVAL_WORK_SECONDS: u64 = 4 * 60 * 60;
pub const MIN_INTERVAL_BREAK_SECONDS: u64 = 60;
pub const MAX_INTERVAL_BREAK_SECONDS: u64 = 60 * 60;
pub const MAX_INTERVAL_REPEATS: u32 = 12;
pub const MAX_INTERVAL_TOTAL_SECONDS: u64 = 12 * 60 * 60;

/// Validated timer configuration. Interval duration includes every work phase
/// and only the breaks between work phases; there is no final break.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionType {
    #[default]
    Infinite,
    Countdown {
        seconds: u64,
    },
    Interval {
        work_seconds: u64,
        break_seconds: u64,
        repeats: u32,
    },
}

impl SessionType {
    pub fn validate(self) -> Result<(), SessionError> {
        match self {
            SessionType::Infinite => Ok(()),
            SessionType::Countdown { seconds }
                if (MIN_COUNTDOWN_SECONDS..=MAX_COUNTDOWN_SECONDS).contains(&seconds) =>
            {
                Ok(())
            }
            SessionType::Countdown { .. } => Err(SessionError::InvalidCountdownConfig),
            SessionType::Interval {
                work_seconds,
                break_seconds,
                repeats,
            } => {
                let total = interval_total_seconds(work_seconds, break_seconds, repeats)
                    .ok_or(SessionError::TimerDurationOverflow)?;
                if !(MIN_INTERVAL_WORK_SECONDS..=MAX_INTERVAL_WORK_SECONDS).contains(&work_seconds)
                    || !(MIN_INTERVAL_BREAK_SECONDS..=MAX_INTERVAL_BREAK_SECONDS)
                        .contains(&break_seconds)
                    || !(1..=MAX_INTERVAL_REPEATS).contains(&repeats)
                {
                    return Err(SessionError::InvalidIntervalConfig);
                }
                if total > MAX_INTERVAL_TOTAL_SECONDS {
                    return Err(SessionError::InvalidIntervalConfig);
                }
                Ok(())
            }
        }
    }

    pub fn total_seconds(self) -> Result<Option<u64>, SessionError> {
        self.validate()?;
        match self {
            SessionType::Infinite => Ok(None),
            SessionType::Countdown { seconds } => Ok(Some(seconds)),
            SessionType::Interval {
                work_seconds,
                break_seconds,
                repeats,
            } => interval_total_seconds(work_seconds, break_seconds, repeats)
                .map(Some)
                .ok_or(SessionError::TimerDurationOverflow),
        }
    }
}

fn interval_total_seconds(work: u64, rest: u64, repeats: u32) -> Option<u64> {
    let rounds = u64::from(repeats);
    work.checked_mul(rounds)?
        .checked_add(rest.checked_mul(rounds.saturating_sub(1))?)
}

/// Lifecycle status of a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    #[default]
    Idle,
    Playing,
    Paused,
    Stopped,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionPhase {
    Work,
    Break,
}

/// Immutable snapshot returned to the UI. The UI never mutates session state
/// directly; it calls commands that return a fresh snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub status: SessionStatus,
    pub activity: Activity,
    pub intensity: Intensity,
    pub kind: SessionType,
    pub phase: Option<SessionPhase>,
    pub current_round: Option<u32>,
    pub total_rounds: Option<u32>,
    pub focus_elapsed_seconds: u64,
    pub current_phase_remaining_seconds: Option<u64>,
    pub total_remaining_seconds: Option<u64>,
}

/// A recoverable focus session.
///
/// Time is supplied by the caller as a monotonic clock reading in seconds
/// (`now`). This keeps the state machine deterministic in tests and lets the
/// Tauri layer bind it to a real `Instant` without the domain depending on a
/// system clock. Elapsed time is only ever accumulated while `Playing`, so a
/// crash or suspend never fabricates focus time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    activity: Activity,
    intensity: Intensity,
    kind: SessionType,
    status: SessionStatus,
    /// Monotonic clock reading at the last committed playing-time boundary.
    phase_start: Option<u64>,
    /// Total active timeline seconds, including interval breaks, accumulated
    /// before `phase_start`. Paused wall time is never included.
    accrued_active: u64,
}

impl Default for Session {
    fn default() -> Self {
        Self::new(Activity::DeepWork, Intensity::Medium, SessionType::Infinite)
            .expect("the default session configuration is valid")
    }
}

impl Session {
    pub fn new(
        activity: Activity,
        intensity: Intensity,
        kind: SessionType,
    ) -> Result<Self, SessionError> {
        kind.validate()?;
        Ok(Self {
            activity,
            intensity,
            kind,
            status: SessionStatus::Idle,
            phase_start: None,
            accrued_active: 0,
        })
    }

    pub fn activity(&self) -> Activity {
        self.activity
    }

    pub fn intensity(&self) -> Intensity {
        self.intensity
    }

    pub fn status(&self) -> SessionStatus {
        self.status
    }

    pub fn session_type(&self) -> SessionType {
        self.kind
    }

    pub fn set_session_type(&mut self, kind: SessionType) -> Result<(), SessionError> {
        kind.validate()?;
        if matches!(self.status, SessionStatus::Playing | SessionStatus::Paused) {
            return Err(SessionError::TimerChangeWhileActive);
        }
        self.kind = kind;
        self.phase_start = None;
        self.accrued_active = 0;
        Ok(())
    }

    /// Select a Focus activity while transport is inactive. Active changes are
    /// rejected so activity and the playing source cannot silently diverge.
    pub fn select_activity(&mut self, activity: Activity) -> Result<(), SessionError> {
        if matches!(self.status, SessionStatus::Playing | SessionStatus::Paused) {
            return Err(SessionError::ActivityChangeWhileActive);
        }
        self.activity = activity;
        Ok(())
    }

    /// Change stimulation intensity. This never changes master volume and is
    /// always allowed without restarting the session.
    pub fn set_intensity(&mut self, intensity: Intensity) {
        self.intensity = intensity;
    }

    /// Begin a new focus session. Returns an error if a session is already
    /// active so the UI cannot silently restart and reset elapsed time.
    pub fn start(&mut self, now: u64) -> Result<(), SessionError> {
        match self.status {
            SessionStatus::Idle | SessionStatus::Stopped | SessionStatus::Expired => {
                self.status = SessionStatus::Playing;
                self.phase_start = Some(now);
                self.accrued_active = 0;
                Ok(())
            }
            _ => Err(SessionError::AlreadyActive),
        }
    }

    /// Restart the active session clock without changing its activity, timer
    /// configuration, stimulation, or audio transport state. Track navigation
    /// uses this so each selected track starts at zero on the player clock.
    pub fn reset_timer(&mut self, now: u64) -> Result<(), SessionError> {
        match self.status {
            SessionStatus::Playing => {
                self.phase_start = Some(now);
                self.accrued_active = 0;
                Ok(())
            }
            SessionStatus::Paused => {
                self.phase_start = None;
                self.accrued_active = 0;
                Ok(())
            }
            _ => Err(SessionError::NotActive),
        }
    }

    pub fn pause(&mut self, now: u64) -> Result<(), SessionError> {
        match self.status {
            SessionStatus::Playing => {
                self.advance_to(now);
                if self.status == SessionStatus::Playing {
                    self.phase_start = None;
                    self.status = SessionStatus::Paused;
                }
                Ok(())
            }
            _ => Err(SessionError::NotPlaying),
        }
    }

    pub fn resume(&mut self, now: u64) -> Result<(), SessionError> {
        match self.status {
            SessionStatus::Paused => {
                self.phase_start = Some(now);
                self.status = SessionStatus::Playing;
                Ok(())
            }
            _ => Err(SessionError::NotPaused),
        }
    }

    pub fn stop(&mut self, now: u64) -> Result<(), SessionError> {
        match self.status {
            SessionStatus::Playing => {
                self.advance_to(now);
                if self.status == SessionStatus::Playing {
                    self.phase_start = None;
                    self.status = SessionStatus::Stopped;
                }
                Ok(())
            }
            SessionStatus::Paused => {
                self.status = SessionStatus::Stopped;
                Ok(())
            }
            _ => Err(SessionError::NotActive),
        }
    }

    fn active_elapsed_seconds(&self, now: u64) -> u64 {
        match self.status {
            SessionStatus::Playing => {
                let phase = self.phase_start.map(|s| now.saturating_sub(s)).unwrap_or(0);
                self.clamp_active(self.accrued_active.saturating_add(phase))
            }
            _ => self.accrued_active,
        }
    }

    fn clamp_active(&self, elapsed: u64) -> u64 {
        self.kind
            .total_seconds()
            .ok()
            .flatten()
            .map_or(elapsed, |total| elapsed.min(total))
    }

    fn advance_to(&mut self, now: u64) {
        if self.status != SessionStatus::Playing {
            return;
        }
        let Some(start) = self.phase_start else {
            return;
        };
        self.accrued_active = self.clamp_active(
            self.accrued_active
                .saturating_add(now.saturating_sub(start)),
        );
        if self
            .kind
            .total_seconds()
            .ok()
            .flatten()
            .is_some_and(|total| self.accrued_active >= total)
        {
            self.phase_start = None;
            self.status = SessionStatus::Expired;
        } else {
            self.phase_start = Some(now.max(start));
        }
    }

    /// Advance the session. A countdown that reaches zero expires. Returns a
    /// snapshot reflecting the post-tick status. Tick never fabricates time:
    /// a paused or stopped session simply reports its frozen elapsed value.
    pub fn tick(&mut self, now: u64) -> SessionSnapshot {
        self.advance_to(now);
        self.snapshot(now)
    }

    pub fn snapshot(&self, now: u64) -> SessionSnapshot {
        let active_elapsed = self.active_elapsed_seconds(now);
        let active = matches!(self.status, SessionStatus::Playing | SessionStatus::Paused);
        let timing = timer_snapshot(self.kind, active_elapsed, active);
        SessionSnapshot {
            status: self.status,
            activity: self.activity,
            intensity: self.intensity,
            kind: self.kind,
            phase: timing.phase,
            current_round: timing.current_round,
            total_rounds: timing.total_rounds,
            focus_elapsed_seconds: timing.focus_elapsed_seconds,
            current_phase_remaining_seconds: timing.current_phase_remaining_seconds,
            total_remaining_seconds: timing.total_remaining_seconds,
        }
    }
}

struct TimerSnapshot {
    phase: Option<SessionPhase>,
    current_round: Option<u32>,
    total_rounds: Option<u32>,
    focus_elapsed_seconds: u64,
    current_phase_remaining_seconds: Option<u64>,
    total_remaining_seconds: Option<u64>,
}

fn timer_snapshot(kind: SessionType, elapsed: u64, active: bool) -> TimerSnapshot {
    match kind {
        SessionType::Infinite => TimerSnapshot {
            phase: active.then_some(SessionPhase::Work),
            current_round: None,
            total_rounds: None,
            focus_elapsed_seconds: elapsed,
            current_phase_remaining_seconds: None,
            total_remaining_seconds: None,
        },
        SessionType::Countdown { seconds } => TimerSnapshot {
            phase: active.then_some(SessionPhase::Work),
            current_round: None,
            total_rounds: None,
            focus_elapsed_seconds: elapsed.min(seconds),
            current_phase_remaining_seconds: active.then(|| seconds.saturating_sub(elapsed)),
            total_remaining_seconds: Some(seconds.saturating_sub(elapsed)),
        },
        SessionType::Interval {
            work_seconds,
            break_seconds,
            repeats,
        } => {
            let total = interval_total_seconds(work_seconds, break_seconds, repeats)
                .expect("validated interval arithmetic");
            let elapsed = elapsed.min(total);
            let cycle = work_seconds
                .checked_add(break_seconds)
                .expect("validated interval arithmetic");
            let completed_cycles = (elapsed / cycle).min(u64::from(repeats));
            let offset = elapsed % cycle;
            let focus_elapsed = completed_cycles
                .saturating_mul(work_seconds)
                .saturating_add(offset.min(work_seconds))
                .min(work_seconds.saturating_mul(u64::from(repeats)));
            let (phase, round, phase_remaining) = if active {
                if offset < work_seconds {
                    (
                        Some(SessionPhase::Work),
                        Some((completed_cycles as u32).saturating_add(1)),
                        Some(work_seconds - offset),
                    )
                } else {
                    (
                        Some(SessionPhase::Break),
                        Some((completed_cycles as u32).saturating_add(1)),
                        Some(cycle - offset),
                    )
                }
            } else {
                (None, None, None)
            };
            TimerSnapshot {
                phase,
                current_round: round,
                total_rounds: Some(repeats),
                focus_elapsed_seconds: focus_elapsed,
                current_phase_remaining_seconds: phase_remaining,
                total_remaining_seconds: Some(total.saturating_sub(elapsed)),
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SessionError {
    #[error("a session is already active")]
    AlreadyActive,
    #[error("the session is not playing")]
    NotPlaying,
    #[error("the session is not paused")]
    NotPaused,
    #[error("no active session to stop")]
    NotActive,
    #[error("stop the active session before changing activity")]
    ActivityChangeWhileActive,
    #[error("stop the active session before changing its timer")]
    TimerChangeWhileActive,
    #[error("countdown duration is outside the supported range")]
    InvalidCountdownConfig,
    #[error("interval work, break, or repeat values are outside the supported range")]
    InvalidIntervalConfig,
    #[error("timer duration arithmetic overflowed")]
    TimerDurationOverflow,
}

#[cfg(test)]
mod master_volume_tests {
    use super::*;

    #[test]
    fn master_volume_is_bounded_with_a_safe_default_and_linear_endpoints() {
        assert_eq!(MasterVolume::default().percent(), 70);
        assert_eq!(MasterVolume::new(0).unwrap().linear_gain(), 0.0);
        assert_eq!(MasterVolume::new(100).unwrap().linear_gain(), 1.0);
        assert_eq!(
            MasterVolume::new(101),
            Err(MasterVolumeError::OutOfRange(101))
        );
    }
}
