use rustfft::{num_complex::Complex, FftPlanner};

const FFT: usize = 2048;
pub const BANDS: usize = 64;

pub struct SpectrumMeter {
    scratch: Vec<Complex<f32>>,
    window: Vec<f32>,
    edges: [usize; BANDS + 1],
    planner_fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
    agc: f32,
}

impl SpectrumMeter {
    pub fn new(sample_rate: u32) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let planner_fft = planner.plan_fft_forward(FFT);
        let window = (0..FFT)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / FFT as f32).cos())
            .collect();
        Self {
            scratch: vec![Complex::new(0.0, 0.0); FFT],
            window,
            edges: band_edges(sample_rate),
            planner_fft,
            agc: 0.15,
        }
    }

    pub fn bands(&mut self, samples: &[f32]) -> [f32; BANDS] {
        let offset = samples.len().saturating_sub(FFT);
        for i in 0..FFT {
            let s = samples.get(offset + i).copied().unwrap_or(0.0);
            self.scratch[i] = Complex::new(s * self.window[i], 0.0);
        }
        self.planner_fft.process(&mut self.scratch);
        let mut raw = [0.0_f32; BANDS];
        for band in 0..BANDS {
            let start = self.edges[band];
            let end = self.edges[band + 1].max(start + 1).min(FFT / 2);
            let mut acc = 0.0;
            let mut count = 0;
            for bin in start..end {
                acc += self.scratch[bin].norm();
                count += 1;
            }
            let mag = if count == 0 { 0.0 } else { acc / count as f32 };
            raw[band] = (mag * 12.0).ln_1p();
        }
        let peak = raw.iter().copied().fold(0.0_f32, f32::max);
        self.agc = (self.agc * 0.92).max(peak);
        let norm = self.agc.max(0.08);
        let mut out = [0.0_f32; BANDS];
        for i in 0..BANDS {
            out[i] = (raw[i] / norm).clamp(0.0, 1.0).powf(0.72);
        }
        out
    }
}

fn band_edges(sample_rate: u32) -> [usize; BANDS + 1] {
    let nyquist = sample_rate as f32 / 2.0;
    let f_min = 32.0_f32;
    let f_max = 16_000.0_f32.min(nyquist * 0.96).max(f_min + 10.0);
    let log_min = f_min.ln();
    let log_max = f_max.ln();
    let mut edges = [0usize; BANDS + 1];
    for i in 0..=BANDS {
        let t = i as f32 / BANDS as f32;
        let freq = (log_min + (log_max - log_min) * t).exp();
        let bin = (freq * FFT as f32 / sample_rate as f32).round() as usize;
        edges[i] = bin.clamp(1, FFT / 2 - 1);
    }
    for i in 1..edges.len() {
        if edges[i] <= edges[i - 1] {
            edges[i] = (edges[i - 1] + 1).min(FFT / 2 - 1);
        }
    }
    edges
}
