//! Stimulation processor with artifact-free parameter smoothing.
//!
//! Implements the isolated interface required by `docs/audio-design.md`:
//! `process(input_pcm, sample_rate, intensity_profile, automation, output_pcm)`.
//! The stateful `Processor` carries parameter smoothers so that intensity
//! changes made during playback ramp instead of jumping, preventing clicks.
//!
//! This is generic amplitude tremolo plus a subtle noise mix. It is NOT a
//! Brain.fm reproduction and makes no medical claim. Patent review is required
//! before any public/commercial neural-stimulation release.

/// Modulation waveform used by the tremolo LFO.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Waveform {
    Sine,
    Triangle,
}

/// Stored DSP profile. The UI exposes only `Intensity`; everything here is an
/// internal configuration that may be tuned without a release.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntensityProfile {
    /// Tremolo rate in Hz.
    pub rate_hz: f32,
    /// Modulation depth in `[0, 1)`. Zero means no modulation (transparent).
    pub depth: f32,
    pub waveform: Waveform,
    /// Wet/dry mix in `[0, 1]`. Zero = fully dry (source only).
    pub mix: f32,
    /// Output gain compensation so intensity does not masquerade as loudness.
    pub output_compensation: f32,
    /// Ramp time (seconds) used when parameters change at runtime.
    pub ramp_time_seconds: f32,
}

impl IntensityProfile {
    /// Off: bit-transparent apart from the shared unity gain.
    pub fn off() -> Self {
        Self {
            rate_hz: 0.0,
            depth: 0.0,
            waveform: Waveform::Sine,
            mix: 0.0,
            output_compensation: 1.0,
            ramp_time_seconds: 0.2,
        }
    }

    pub fn low() -> Self {
        Self {
            rate_hz: 8.0,
            depth: 0.18,
            waveform: Waveform::Sine,
            mix: 0.6,
            output_compensation: 1.057,
            ramp_time_seconds: 0.3,
        }
    }

    pub fn medium() -> Self {
        Self {
            rate_hz: 12.0,
            depth: 0.36,
            waveform: Waveform::Sine,
            mix: 0.8,
            output_compensation: 1.168,
            ramp_time_seconds: 0.3,
        }
    }

    pub fn high() -> Self {
        Self {
            rate_hz: 16.0,
            depth: 0.54,
            waveform: Waveform::Sine,
            mix: 1.0,
            output_compensation: 1.370,
            ramp_time_seconds: 0.3,
        }
    }

    pub fn for_intensity(intensity: domain_shim::Intensity) -> Self {
        use domain_shim::Intensity::*;
        match intensity {
            Off => Self::off(),
            Low => Self::low(),
            Medium => Self::medium(),
            High => Self::high(),
        }
    }
}

/// Tiny local shim so this crate does not depend on `domain`. The Tauri layer
/// maps the real `domain::Intensity` to these values; the DSP stays decoupled
/// from session state, as the architecture requires.
pub mod domain_shim {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Intensity {
        Off,
        Low,
        Medium,
        High,
    }
}

/// One-pole parameter smoother. Time constant chosen so a step change ramps
/// over roughly `ramp_time_seconds` without discontinuity.
#[derive(Debug, Clone, Copy)]
struct Smoother {
    value: f32,
    target: f32,
    coeff: f32,
}

impl Smoother {
    fn new(initial: f32, ramp_time_seconds: f32, sample_rate: u32) -> Self {
        Self {
            value: initial,
            target: initial,
            coeff: smoothing_coeff(ramp_time_seconds, sample_rate),
        }
    }

    fn set_target(&mut self, target: f32, ramp_time_seconds: f32, sample_rate: u32) {
        self.target = target;
        self.coeff = smoothing_coeff(ramp_time_seconds, sample_rate);
    }

    #[inline]
    fn tick(&mut self) -> f32 {
        // Classic one-pole low-pass: value += coeff * (target - value).
        // Per-sample, so the trajectory is continuous (no click).
        self.value += self.coeff * (self.target - self.value);
        self.value
    }
}

fn smoothing_coeff(ramp_time_seconds: f32, sample_rate: u32) -> f32 {
    if ramp_time_seconds <= 0.0 {
        return 1.0; // instant, still single-step bounded
    }
    let tc = ramp_time_seconds * sample_rate as f32;
    // For value += coeff * (target - value), this coefficient reaches
    // 1 - exp(-1) ~= 63.2% of a step after one time constant. `exp_m1`
    // avoids cancellation for long ramps where the coefficient is tiny.
    let c = -(-1.0 / tc.max(1.0)).exp_m1();
    c.clamp(0.0, 1.0)
}

/// Deterministic noise generator (LCG). Determinism is required for repeatable
/// DSP tests; it is not cryptographically relevant.
#[derive(Debug)]
struct Lcg {
    state: u32,
}

impl Lcg {
    fn new(seed: u32) -> Self {
        Self { state: seed }
    }
    #[inline]
    fn next_f32(&mut self) -> f32 {
        // Numerical Recipes LCG constants.
        self.state = self
            .state
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        // Map to [-1, 1).
        ((self.state >> 8) as f32 / 8_388_607.5) - 1.0
    }
}

/// Stateful stimulation processor. Cheap to construct; safe to keep alive for
/// the duration of a session.
#[derive(Debug)]
pub struct Processor {
    sample_rate: u32,
    phase: f32,
    noise: Lcg,
    depth: Smoother,
    mix: Smoother,
    compensation: Smoother,
    rate_hz: f32,
    waveform: Waveform,
}

impl Processor {
    pub fn new(sample_rate: u32, initial: IntensityProfile) -> Self {
        Self {
            sample_rate,
            phase: 0.0,
            noise: Lcg::new(0xC0FFEE),
            depth: Smoother::new(initial.depth, initial.ramp_time_seconds, sample_rate),
            mix: Smoother::new(initial.mix, initial.ramp_time_seconds, sample_rate),
            compensation: Smoother::new(
                initial.output_compensation,
                initial.ramp_time_seconds,
                sample_rate,
            ),
            rate_hz: initial.rate_hz,
            waveform: initial.waveform,
        }
    }

    /// Schedule a new target profile. Parameters ramp over `ramp_time_seconds`,
    /// so calling this during playback does not produce a click.
    pub fn set_target(&mut self, profile: IntensityProfile) {
        self.depth
            .set_target(profile.depth, profile.ramp_time_seconds, self.sample_rate);
        self.mix
            .set_target(profile.mix, profile.ramp_time_seconds, self.sample_rate);
        self.compensation.set_target(
            profile.output_compensation,
            profile.ramp_time_seconds,
            self.sample_rate,
        );
        // Rate and waveform switch instantly; depth smoothing hides any
        // audible seam because the modulator amplitude is ramped.
        self.rate_hz = profile.rate_hz;
        self.waveform = profile.waveform;
    }

    /// Process one mono sample. This is the allocation-free primitive used by
    /// the native audio callback.
    #[inline]
    pub fn process_sample(&mut self, input: f32) -> f32 {
        let sr = self.sample_rate as f32;
        let depth = self.depth.tick();
        let mix = self.mix.tick();
        let comp = self.compensation.tick();

        // Tremolo modulator in [1 - depth, 1].
        let m = match self.waveform {
            Waveform::Sine => 1.0 - depth * (0.5 - 0.5 * self.phase.sin()),
            Waveform::Triangle => {
                let ph = self.phase / (2.0 * std::f32::consts::PI);
                let tri = 1.0 - 2.0 * (ph.fract() - 0.5).abs() * 2.0;
                1.0 - depth * (0.5 - 0.5 * tri)
            }
        };
        // Subtle wideband noise mixed in proportion to depth.
        let noise = self.noise.next_f32() * depth * 0.01;
        let wet = input * m + noise;
        let output = (input * (1.0 - mix) + wet * mix) * comp;

        self.phase += 2.0 * std::f32::consts::PI * self.rate_hz / sr;
        if self.phase > 2.0 * std::f32::consts::PI {
            self.phase -= 2.0 * std::f32::consts::PI;
        }
        output
    }

    /// Process a block of mono samples in place into `output`.
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) {
        for (&input, output) in input.iter().zip(output.iter_mut()) {
            *output = self.process_sample(input);
        }
        // If output is longer than input, leave the tail untouched.
    }

    /// Current smoothed depth (for tests and metering).
    pub fn current_depth(&self) -> f32 {
        self.depth.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sustained_tone(sr: u32, hz: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| 0.25 * (2.0 * std::f32::consts::PI * hz * i as f32 / sr as f32).sin())
            .collect()
    }

    /// Peak-to-peak modulation metric. Each window holds an integer number of
    /// carrier cycles, so for an unmodulated tone the per-window peak-to-peak
    /// amplitude is constant and the metric is ~0. Tremolo makes it scale with
    /// depth, giving a clean distinctness measure.
    fn modulation_metric(out: &[f32], window: usize) -> f32 {
        let p2p: Vec<f32> = out
            .chunks_exact(window)
            .map(|w| {
                let max = w.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                let min = w.iter().cloned().fold(f32::INFINITY, f32::min);
                max - min
            })
            .collect();
        if p2p.is_empty() {
            return 0.0;
        }
        let mean = p2p.iter().sum::<f32>() / p2p.len() as f32;
        if mean <= 1e-9 {
            return 0.0;
        }
        let (min, max) = p2p
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(mn, mx), &v| {
                (mn.min(v), mx.max(v))
            });
        (max - min) / mean
    }

    #[test]
    fn smoother_follows_its_time_constant_without_jump_or_overshoot() {
        let sample_rate = 1_000;
        let ramp_seconds = 0.1;
        let samples_per_time_constant = (sample_rate as f32 * ramp_seconds) as usize;
        let mut smoother = Smoother::new(0.0, ramp_seconds, sample_rate);
        smoother.set_target(1.0, ramp_seconds, sample_rate);

        let first = smoother.tick();
        assert!(
            first > 0.0 && first < 0.02,
            "first sample moved too far: {first}"
        );

        let mut previous = first;
        for _ in 1..samples_per_time_constant {
            let current = smoother.tick();
            assert!(current >= previous, "smoother was not monotonic");
            assert!(current <= 1.0, "smoother overshot target: {current}");
            previous = current;
        }

        let expected_one_time_constant = 1.0 - (-1.0f32).exp();
        assert!(
            (previous - expected_one_time_constant).abs() < 0.002,
            "after one time constant: {previous}, expected {expected_one_time_constant}"
        );

        for _ in 0..samples_per_time_constant * 9 {
            let current = smoother.tick();
            assert!(current >= previous, "smoother was not monotonic");
            assert!(current <= 1.0, "smoother overshot target: {current}");
            previous = current;
        }
        assert!(previous > 0.999, "smoother did not converge: {previous}");
    }

    #[test]
    fn off_is_bit_transparent() {
        let sr = 44_100;
        let input = sustained_tone(sr, 220.0, sr as usize);
        let mut proc = Processor::new(sr, IntensityProfile::off());
        let mut out = vec![0.0; input.len()];
        proc.process(&input, &mut out);
        for (a, b) in input.iter().zip(out.iter()) {
            assert!((a - b).abs() <= 1e-6, "Off diverged: {} vs {}", a, b);
        }
    }

    #[test]
    fn low_medium_high_are_distinct() {
        let sr = 44_100;
        // 100 Hz gives an integer 441-sample carrier period; a 1764-sample
        // window is exactly 4 carrier cycles, so the metric isolates tremolo.
        let input = sustained_tone(sr, 100.0, sr as usize); // 1s
        let window = 1_764;

        let metric = |profile: IntensityProfile| {
            let mut p = Processor::new(sr, profile);
            let mut o = vec![0.0; input.len()];
            p.process(&input, &mut o);
            modulation_metric(&o, window)
        };

        let off = metric(IntensityProfile::off());
        let low = metric(IntensityProfile::low());
        let med = metric(IntensityProfile::medium());
        let high = metric(IntensityProfile::high());

        assert!(off < 0.03, "off={off}");
        assert!(low > off, "low={low} off={off}");
        assert!(low < med, "low={low} med={med}");
        assert!(med < high, "med={med} high={high}");
    }

    #[test]
    fn intensity_change_does_not_click() {
        let sr = 44_100;
        let input = sustained_tone(sr, 220.0, sr as usize * 2); // 2s
        let mut proc = Processor::new(sr, IntensityProfile::off());
        let mut out = vec![0.0; input.len()];
        // Switch near a carrier peak rather than at the favorable zero crossing
        // at exactly one second. This makes a parameter jump readily visible.
        let quarter_cycle = (sr as f32 / (4.0 * 220.0)).round() as usize;
        let switch = sr as usize + quarter_cycle;
        assert!(input[switch].abs() > 0.24, "switch was not near a peak");

        proc.process(&input[..switch], &mut out[..switch]);
        proc.set_target(IntensityProfile::high());
        proc.process(&input[switch..], &mut out[switch..]);

        // Remove the carrier's ordinary sample-to-sample movement and measure
        // only the discontinuity introduced by changing the DSP parameters.
        let dry_step = input[switch] - input[switch - 1];
        let processed_step = out[switch] - out[switch - 1];
        let transition_discontinuity = (processed_step - dry_step).abs();
        assert!(
            transition_discontinuity < 0.0005,
            "click-like transition discontinuity={transition_discontinuity}"
        );

        // The smoothed depth must rise toward the target and never overshoot it.
        assert!(proc.current_depth() > 0.0 && proc.current_depth() <= 0.54 + 1e-4);
    }

    #[test]
    fn output_compensation_prevents_loudness_masquerading() {
        let sr = 44_100;
        let input = sustained_tone(sr, 220.0, sr as usize);
        let rms = |o: &[f32]| (o.iter().map(|x| x * x).sum::<f32>() / o.len() as f32).sqrt();
        let profiles = [
            IntensityProfile::off(),
            IntensityProfile::low(),
            IntensityProfile::medium(),
            IntensityProfile::high(),
        ];
        let mut rmses = Vec::new();
        for p in profiles {
            let mut proc = Processor::new(sr, p);
            let mut o = vec![0.0; input.len()];
            proc.process(&input, &mut o);
            rmses.push(rms(&o));
        }
        // Compensation keeps perceived loudness flat: higher intensity must not
        // be louder than Off, and the whole band stays tight.
        let off = rmses[0];
        for r in &rmses {
            assert!(*r <= off * 1.10, "intensity louder than Off: {r} > {off}");
        }
        let lo = rmses.iter().cloned().fold(f32::INFINITY, f32::min);
        let hi = rmses.iter().cloned().fold(0.0f32, f32::max);
        assert!(hi / lo < 1.15, "loudness spread too wide: {lo}..{hi}");
    }
}
