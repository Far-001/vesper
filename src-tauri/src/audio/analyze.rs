use std::path::Path;

use rustfft::{num_complex::Complex, FftPlanner};

use super::decode::decode_mono_prefix;

#[derive(Clone, Copy, Debug)]
pub struct Features {
    pub bpm: f32,
    pub energy: f32,
    pub centroid: f32,
}

pub fn analyze_file(path: &Path) -> Result<Features, String> {
    let (mono, rate) = decode_mono_prefix(path, 45.0)?;
    Ok(features_from_pcm(&mono, rate))
}

pub fn features_from_pcm(mono: &[f32], rate: u32) -> Features {
    if mono.len() < rate as usize / 2 || rate == 0 {
        return Features { bpm: 0.0, energy: 0.0, centroid: 0.0 };
    }
    Features {
        bpm: sanitize(estimate_bpm(mono, rate)),
        energy: sanitize(rms(mono)),
        centroid: sanitize(spectral_centroid(mono, rate)),
    }
}

fn sanitize(value: f32) -> f32 {
    if value.is_finite() && value >= 0.0 {
        value
    } else {
        0.0
    }
}

fn rms(mono: &[f32]) -> f32 {
    let sum: f32 = mono.iter().map(|sample| sample * sample).sum();
    (sum / mono.len() as f32).sqrt()
}

fn spectral_centroid(mono: &[f32], rate: u32) -> f32 {
    let frame = 2048usize;
    if mono.len() < frame {
        return 0.0;
    }
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(frame);
    let mut acc = 0.0;
    let mut frames = 0.0;
    let mut start = 0;
    while start + frame < mono.len() && frames < 40.0 {
        let mut buf: Vec<Complex<f32>> = (0..frame)
            .map(|i| {
                let window = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / frame as f32).cos();
                Complex::new(mono[start + i] * window, 0.0)
            })
            .collect();
        fft.process(&mut buf);
        let mut num = 0.0;
        let mut den = 0.0;
        for bin in 1..frame / 2 {
            let mag = buf[bin].norm();
            let freq = bin as f32 * rate as f32 / frame as f32;
            num += freq * mag;
            den += mag;
        }
        if den > 0.0 {
            acc += num / den;
            frames += 1.0;
        }
        start += frame;
    }
    if frames == 0.0 {
        0.0
    } else {
        acc / frames
    }
}

fn estimate_bpm(mono: &[f32], rate: u32) -> f32 {
    let frame = 1024usize;
    let hop = 512usize;
    if mono.len() < frame * 4 {
        return 0.0;
    }
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(frame);
    let mut prev = vec![0.0_f32; frame / 2];
    let mut flux = Vec::new();
    let mut start = 0;
    while start + frame < mono.len() {
        let mut buf: Vec<Complex<f32>> = (0..frame)
            .map(|i| {
                let window = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / frame as f32).cos();
                Complex::new(mono[start + i] * window, 0.0)
            })
            .collect();
        fft.process(&mut buf);
        let mut mag = vec![0.0_f32; frame / 2];
        let mut diff = 0.0;
        for bin in 0..frame / 2 {
            mag[bin] = buf[bin].norm();
            let delta = mag[bin] - prev[bin];
            if delta > 0.0 {
                diff += delta;
            }
        }
        prev = mag;
        flux.push(diff);
        start += hop;
    }
    if flux.len() < 16 {
        return 0.0;
    }
    let mean = flux.iter().sum::<f32>() / flux.len() as f32;
    for value in &mut flux {
        *value = (*value - mean).max(0.0);
    }
    let hop_hz = rate as f32 / hop as f32;
    let min_bpm = 70.0;
    let max_bpm = 180.0;
    let min_lag = (hop_hz * 60.0 / max_bpm).round().max(1.0) as usize;
    let max_lag = (hop_hz * 60.0 / min_bpm).round() as usize;
    let max_lag = max_lag.min(flux.len() / 2).max(min_lag);
    let mut best_lag = 0;
    let mut best = 0.0;
    for lag in min_lag..=max_lag {
        let mut corr = 0.0;
        let mut count = 0.0;
        let mut index = 0;
        while index + lag < flux.len() {
            corr += flux[index] * flux[index + lag];
            count += 1.0;
            index += 1;
        }
        if count > 0.0 {
            corr /= count;
        }
        if corr > best {
            best = corr;
            best_lag = lag;
        }
    }
    if best_lag == 0 || best <= 0.0 {
        return 0.0;
    }
    (60.0 * hop_hz / best_lag as f32).clamp(min_bpm, max_bpm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::decode::write_wav_mono;

    fn clicks(bpm: f32, secs: f32, rate: u32) -> Vec<f32> {
        let mut samples = vec![0.0; (secs * rate as f32) as usize];
        let interval = ((rate as f32) * 60.0 / bpm).round().max(1.0) as usize;
        let mut at = 0;
        while at < samples.len() {
            for k in 0..180 {
                if at + k >= samples.len() {
                    break;
                }
                let env = 1.0 - k as f32 / 180.0;
                samples[at + k] += env * (k as f32 * 0.35).sin();
            }
            at += interval;
        }
        samples
    }

    #[test]
    fn bpm_of_click_track_is_close() {
        let rate = 22_050;
        let mono = clicks(120.0, 12.0, rate);
        let features = features_from_pcm(&mono, rate);
        assert!(
            (features.bpm - 120.0).abs() < 3.0,
            "bpm {}",
            features.bpm
        );
    }

    #[test]
    fn bright_tone_has_higher_centroid_than_dark() {
        let rate = 22_050;
        let dark: Vec<f32> = (0..rate as usize * 2)
            .map(|i| (2.0 * std::f32::consts::PI * 120.0 * i as f32 / rate as f32).sin() * 0.5)
            .collect();
        let bright: Vec<f32> = (0..rate as usize * 2)
            .map(|i| (2.0 * std::f32::consts::PI * 4000.0 * i as f32 / rate as f32).sin() * 0.5)
            .collect();
        let dark_c = features_from_pcm(&dark, rate).centroid;
        let bright_c = features_from_pcm(&bright, rate).centroid;
        assert!(bright_c > dark_c + 500.0, "dark {dark_c} bright {bright_c}");
    }

    #[test]
    fn analyze_file_reads_wav() {
        let dir = std::env::temp_dir().join(format!("vesper-an-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("clicks.wav");
        let rate = 22_050;
        write_wav_mono(&path, &clicks(100.0, 12.0, rate), rate).unwrap();
        let features = analyze_file(&path).unwrap();
        assert!((features.bpm - 100.0).abs() < 4.0, "bpm {}", features.bpm);
        assert!(features.energy > 0.0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
