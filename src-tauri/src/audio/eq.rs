//! 10-полосный графический эквалайзер. Коэффициенты — peaking biquad (RBJ).

pub const BANDS_HZ: [f32; 10] = [
    32.0, 64.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];

const Q: f32 = 1.414;

#[derive(Clone, Copy)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn identity() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn peaking(sample_rate: f32, freq: f32, gain_db: f32) -> Self {
        let nyquist = sample_rate * 0.45;
        if freq >= nyquist || gain_db.abs() < 0.05 {
            return Self::identity();
        }
        let a = 10.0_f32.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * freq / sample_rate;
        let alpha = w0.sin() / (2.0 * Q);
        let cos_w = w0.cos();
        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos_w;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cos_w;
        let a2 = 1.0 - alpha / a;
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        if y.abs() < 1.0e-15 {
            0.0
        } else {
            y
        }
    }
}

pub struct EqChain {
    sample_rate: u32,
    gains: [f32; 10],
    left: [Biquad; 10],
    right: [Biquad; 10],
}

impl EqChain {
    pub fn new(sample_rate: u32) -> Self {
        let mut eq = Self {
            sample_rate,
            gains: [0.0; 10],
            left: [Biquad::identity(); 10],
            right: [Biquad::identity(); 10],
        };
        eq.rebuild(true);
        eq
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        if self.sample_rate != sample_rate {
            self.sample_rate = sample_rate;
            self.rebuild(true);
        }
    }

    pub fn set_gains(&mut self, gains: [f32; 10]) {
        if gains != self.gains {
            self.gains = gains;
            self.rebuild(false);
        }
    }

    pub fn reset_state(&mut self) {
        for band in self.left.iter_mut().chain(self.right.iter_mut()) {
            band.z1 = 0.0;
            band.z2 = 0.0;
        }
    }

    pub fn process_interleaved(&mut self, samples: &mut [f32]) {
        for frame in samples.chunks_exact_mut(2) {
            let mut l = frame[0];
            let mut r = frame[1];
            for i in 0..10 {
                l = self.left[i].process(l);
                r = self.right[i].process(r);
            }
            frame[0] = l.clamp(-1.0, 1.0);
            frame[1] = r.clamp(-1.0, 1.0);
        }
    }

    fn rebuild(&mut self, reset_state: bool) {
        let rate = self.sample_rate.max(8000) as f32;
        for i in 0..10 {
            let mut left = Biquad::peaking(rate, BANDS_HZ[i], self.gains[i]);
            let mut right = Biquad::peaking(rate, BANDS_HZ[i], self.gains[i]);
            if !reset_state {
                left.z1 = self.left[i].z1;
                left.z2 = self.left[i].z2;
                right.z1 = self.right[i].z1;
                right.z2 = self.right[i].z2;
            }
            self.left[i] = left;
            self.right[i] = right;
        }
    }
}

pub fn preset(name: &str) -> Option<[f32; 10]> {
    let bands = match name {
        "flat" => [0.0; 10],
        "rock" => [4.0, 3.0, 1.5, 0.0, -1.0, -1.0, 1.0, 2.5, 3.5, 4.0],
        "bass" => [7.0, 5.5, 4.0, 2.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0],
        "vocal" => [-2.0, -1.5, 0.0, 2.0, 4.0, 4.0, 2.5, 1.0, 0.0, -1.5],
        "night" => [1.5, 1.0, 0.5, 0.0, 0.0, -0.5, -1.5, -3.0, -4.0, -5.0],
        _ => return None,
    };
    Some(bands)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum: f32 = samples.iter().map(|s| s * s).sum();
        (sum / samples.len() as f32).sqrt()
    }

    #[test]
    fn flat_eq_keeps_level() {
        let mut eq = EqChain::new(48_000);
        let mut samples = Vec::new();
        for i in 0..48_000 {
            let s = (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 48_000.0).sin() * 0.4;
            samples.push(s);
            samples.push(s);
        }
        let before = rms(&samples);
        eq.process_interleaved(&mut samples);
        let after = rms(&samples[4000..]);
        assert!((after - before).abs() < 0.02, "before {before} after {after}");
    }

    #[test]
    fn plus_six_db_roughly_doubles_tone_at_band() {
        let mut eq = EqChain::new(48_000);
        let mut gains = [0.0; 10];
        gains[5] = 6.0;
        eq.set_gains(gains);
        let mut samples = Vec::new();
        for i in 0..48_000 {
            let s = (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 48_000.0).sin() * 0.2;
            samples.push(s);
            samples.push(s);
        }
        let before = rms(&samples);
        eq.process_interleaved(&mut samples);
        let after = rms(&samples[8000..]);
        let ratio = after / before;
        assert!(ratio > 1.6 && ratio < 2.4, "ratio {ratio}");
    }
}
