//! Жадный обход ближайших соседей: темп, энергия, яркость.
//! Это порядок для слушания, не битматч.

#[derive(Clone, Copy, Debug)]
pub struct Feat {
    pub bpm: f32,
    pub energy: f32,
    pub centroid: f32,
}

struct Span {
    min: f32,
    max: f32,
}

impl Span {
    fn from_values(values: impl Iterator<Item = f32>) -> Self {
        let mut min = f32::MAX;
        let mut max = f32::MIN;
        let mut any = false;
        for v in values {
            any = true;
            min = min.min(v);
            max = max.max(v);
        }
        if !any {
            Self { min: 0.0, max: 0.0 }
        } else {
            Self { min, max }
        }
    }

    fn norm(&self, v: f32) -> f32 {
        let width = self.max - self.min;
        if width < 1.0e-4 {
            0.0
        } else {
            (v - self.min) / width
        }
    }
}

struct XorShift(u64);

impl XorShift {
    fn unit(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x & 0x00ff_ffff) as f32 / 16_777_215.0
    }
}

pub fn build_radio(seed: usize, feats: &[Feat], want: usize, rng_state: u64) -> Vec<(usize, String)> {
    if feats.is_empty() {
        return Vec::new();
    }
    let seed = seed.min(feats.len() - 1);
    let want = want.clamp(1, 30).min(feats.len());
    let bpm = Span::from_values(feats.iter().filter(|f| f.bpm > 0.0).map(|f| f.bpm));
    let energy = Span::from_values(feats.iter().map(|f| f.energy));
    let centroid = Span::from_values(feats.iter().map(|f| f.centroid));
    let mut rng = XorShift(rng_state | 1);
    let ban = if feats.len() <= 2 { 0 } else { 4.min(feats.len() - 1) };

    let mut chosen = vec![seed];
    let mut reasons = vec!["старт эфира".to_string()];

    while chosen.len() < want {
        let prev_i = *chosen.last().unwrap();
        let mut best: Option<(usize, f32)> = None;
        for i in 0..feats.len() {
            if chosen.iter().rev().take(ban).any(|&c| c == i) {
                continue;
            }
            if chosen.contains(&i) {
                continue;
            }
            let mut distance = feature_distance(&bpm, &energy, &centroid, &feats[prev_i], &feats[i]);
            distance += rng.unit() * 0.02;
            if best.map(|(_, best_d)| distance < best_d).unwrap_or(true) {
                best = Some((i, distance));
            }
        }
        let Some((next_i, _)) = best else {
            break;
        };
        reasons.push(explain(&feats[prev_i], &feats[next_i]));
        chosen.push(next_i);
    }

    chosen.into_iter().zip(reasons).collect()
}

fn feature_distance(bpm: &Span, energy: &Span, centroid: &Span, a: &Feat, b: &Feat) -> f32 {
    let mut distance = 0.0;
    if a.bpm > 0.0 && b.bpm > 0.0 {
        let delta = bpm.norm(a.bpm) - bpm.norm(b.bpm);
        distance += 0.50 * delta * delta;
    }
    let de = energy.norm(a.energy) - energy.norm(b.energy);
    distance += 0.30 * de * de;
    let dc = centroid.norm(a.centroid) - centroid.norm(b.centroid);
    distance += 0.20 * dc * dc;
    distance
}

fn explain(prev: &Feat, next: &Feat) -> String {
    let mut parts = Vec::new();
    if prev.bpm > 0.0 && next.bpm > 0.0 {
        parts.push(format!("темп {:.0} → {:.0}", prev.bpm.round(), next.bpm.round()));
    }
    let energy = next.energy - prev.energy;
    parts.push(
        if energy > 0.02 {
            "энергия выше"
        } else if energy < -0.02 {
            "энергия ниже"
        } else {
            "энергия рядом"
        }
        .to_string(),
    );
    let tone = next.centroid - prev.centroid;
    parts.push(
        if tone > 400.0 {
            "ярче"
        } else if tone < -400.0 {
            "темнее"
        } else {
            "та же окраска"
        }
        .to_string(),
    );
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbor_prefers_close_tempo() {
        let feats = vec![
            Feat { bpm: 100.0, energy: 0.2, centroid: 1500.0 },
            Feat { bpm: 102.0, energy: 0.21, centroid: 1550.0 },
            Feat { bpm: 168.0, energy: 0.22, centroid: 1600.0 },
        ];
        let order = build_radio(0, &feats, 3, 1);
        assert_eq!(order[0].0, 0);
        assert_eq!(order[0].1, "старт эфира");
        assert_eq!(order[1].0, 1);
        assert!(order[1].1.contains("темп"));
    }

    #[test]
    fn empty_pool_is_empty() {
        assert!(build_radio(0, &[], 10, 1).is_empty());
    }
}
