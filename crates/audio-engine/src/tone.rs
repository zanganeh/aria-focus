//! Procedural generation of a safe, licence-clean test tone.
//!
//! The "Deep Work pad" is a soft, slowly evolving sine drone with no vocals,
//! speech, hooks, drops, or abrupt changes. It is exactly periodic over
//! `duration_seconds`, so looping is seamless with no crossfade needed.
//! All partials and the LFO are integer multiples of `1 / duration_seconds`.

use crate::Pcm;

/// Configuration for the bundled test tone.
#[derive(Debug, Clone, Copy)]
pub struct ToneConfig {
    pub sample_rate: u32,
    pub duration_seconds: f32,
    /// Fundamental frequency (Hz). Must satisfy `fundamental * duration_seconds`
    /// is an integer so the buffer is exactly periodic.
    pub fundamental_hz: f32,
}

impl Default for ToneConfig {
    fn default() -> Self {
        // 8 s loop, A2 = 110 Hz. 110 * 8 = 880 (integer) => seamless loop.
        Self {
            sample_rate: crate::DEFAULT_SAMPLE_RATE,
            duration_seconds: 8.0,
            fundamental_hz: 110.0,
        }
    }
}

/// Additive chord partials: (multiplier, amplitude).
const PARTIALS: [(f32, f32); 4] = [(1.0, 0.30), (1.5, 0.18), (2.0, 0.12), (3.0, 0.06)];

/// Evaluate one sample of the pad at time `t` seconds.
fn sample_at(cfg: ToneConfig, t: f32) -> f32 {
    let f0 = cfg.fundamental_hz;
    let lfo_freq = 1.0 / cfg.duration_seconds; // one cycle per loop
    let mut sample = 0.0f32;
    for (mult, amp) in PARTIALS {
        sample += amp * (2.0 * std::f32::consts::PI * f0 * mult * t).sin();
    }
    let lfo = 0.88 + 0.12 * (2.0 * std::f32::consts::PI * lfo_freq * t).sin();
    sample *= lfo;
    // Master trim to ~-12 dBFS peak leaving headroom for processing.
    sample * 0.25
}

/// Allocation-free live source used by the native output callback.
///
/// The sample cursor wraps at the authored loop boundary. Construction and
/// reset happen on the control thread; `next_sample` performs only arithmetic.
#[derive(Debug)]
pub(crate) struct ToneSource {
    config: ToneConfig,
    sample_index: u64,
    loop_samples: u64,
}

impl ToneSource {
    pub(crate) fn new(sample_rate: u32) -> Self {
        let config = ToneConfig {
            sample_rate,
            ..ToneConfig::default()
        };
        let loop_samples = (config.sample_rate as f32 * config.duration_seconds).round() as u64;
        Self {
            config,
            sample_index: 0,
            loop_samples: loop_samples.max(1),
        }
    }

    #[inline]
    pub(crate) fn next_sample(&mut self) -> f32 {
        let t = self.sample_index as f32 / self.config.sample_rate as f32;
        let sample = sample_at(self.config, t);
        self.sample_index += 1;
        if self.sample_index == self.loop_samples {
            self.sample_index = 0;
        }
        sample
    }
}

/// Generate a mono floating-point test tone in `[-1, 1)`.
///
/// The signal is a quiet additive chord (root, fifth, octave, twelfth) with a
/// slow one-cycle-per-loop amplitude LFO for gentle evolution. Peak amplitude
/// is kept well below 0 dB to leave headroom for stimulation processing and the
/// output limiter.
pub fn generate_test_tone(cfg: ToneConfig) -> Pcm {
    let n = (cfg.sample_rate as f32 * cfg.duration_seconds).round() as usize;
    let sr = cfg.sample_rate as f32;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        out.push(sample_at(cfg, i as f32 / sr));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tone_is_bounded_and_non_silent() {
        let pcm = generate_test_tone(ToneConfig::default());
        let peak = pcm.iter().fold(0.0f32, |a, &x| a.max(x.abs()));
        assert!(peak > 0.05 && peak < 0.5, "peak={peak}");
        assert!(pcm.iter().any(|&x| x.abs() > 1e-3));
    }

    #[test]
    fn tone_is_exactly_periodic_over_the_loop() {
        // The buffer is periodic with period `duration_seconds`, so the sample
        // just past the end (t = duration) must equal the first sample, and the
        // slope across the wrap must match the slope at the start.
        let cfg = ToneConfig::default();
        let pcm = generate_test_tone(cfg);
        let wrap = sample_at(cfg, cfg.duration_seconds);
        let start = pcm[0];
        assert!(
            (wrap - start).abs() < 1e-4,
            "value wrap mismatch: {wrap} vs {start}"
        );
        let slope_wrap = wrap - pcm[pcm.len() - 1];
        let slope_start = pcm[1] - pcm[0];
        assert!(
            (slope_wrap - slope_start).abs() < 1e-3,
            "slope mismatch across loop: {slope_wrap} vs {slope_start}"
        );
    }

    #[test]
    fn tone_has_no_transient_jumps() {
        let pcm = generate_test_tone(ToneConfig::default());
        let max_jump = pcm
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        // A soft pad may have ordinary sample-to-sample steps from its partials,
        // but no transient/click spike. 0.02 is far below a click (>0.1).
        assert!(max_jump < 0.02, "max_jump={max_jump}");
    }

    #[test]
    fn live_source_wraps_at_the_same_seam() {
        let cfg = ToneConfig::default();
        let mut source = ToneSource::new(cfg.sample_rate);
        let samples = (cfg.sample_rate as f32 * cfg.duration_seconds).round() as usize;
        let first = source.next_sample();
        for _ in 1..samples {
            let _ = source.next_sample();
        }
        let wrapped = source.next_sample();
        assert!((first - wrapped).abs() < 1e-6);
    }
}
