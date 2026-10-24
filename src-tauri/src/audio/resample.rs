//! Pure DSP helpers: resample mono f32 audio to 16 kHz, convert to i16, detect speech.

pub const TARGET_RATE: u32 = 16_000;

const TAPS: usize = 63; // odd → symmetric, integer group delay

/// Windowed-sinc low-pass (Hamming), `fc` in cycles/sample (0..0.5).
fn lowpass_kernel(fc: f64) -> [f32; TAPS] {
    let mut k = [0f32; TAPS];
    let m = (TAPS - 1) as f64;
    let mut sum = 0.0;
    for (i, v) in k.iter_mut().enumerate() {
        let n = i as f64 - m / 2.0;
        let sinc = if n == 0.0 { 2.0 * fc } else { (2.0 * std::f64::consts::PI * fc * n).sin() / (std::f64::consts::PI * n) };
        let w = 0.54 - 0.46 * (2.0 * std::f64::consts::PI * i as f64 / m).cos();
        let x = sinc * w;
        *v = x as f32;
        sum += x;
    }
    for v in k.iter_mut() {
        *v = (*v as f64 / sum) as f32; // unity DC gain
    }
    k
}

#[inline]
fn filtered_at(input: &[f32], k: &[f32; TAPS], center: isize) -> f32 {
    let half = (TAPS / 2) as isize;
    let mut acc = 0f32;
    for (j, kv) in k.iter().enumerate() {
        let idx = center + j as isize - half;
        if idx >= 0 && (idx as usize) < input.len() {
            acc += input[idx as usize] * kv;
        }
    }
    acc
}

/// Resample mono audio from `in_rate` to 16 kHz.
/// Downsampling applies an anti-alias low-pass (~7.6 kHz), evaluated only at the
/// samples we need, then linear interpolation between them.
pub fn to_16k(input: &[f32], in_rate: u32) -> Vec<f32> {
    if in_rate == TARGET_RATE || input.is_empty() {
        return input.to_vec();
    }
    let ratio = in_rate as f64 / TARGET_RATE as f64;
    let out_len = ((input.len() as f64) / ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);

    if in_rate > TARGET_RATE {
        let fc = 7_600.0 / in_rate as f64;
        let k = lowpass_kernel(fc);
        for i in 0..out_len {
            let pos = i as f64 * ratio;
            let i0 = pos.floor() as isize;
            let frac = (pos - i0 as f64) as f32;
            let a = filtered_at(input, &k, i0);
            let b = if frac > 0.0 { filtered_at(input, &k, i0 + 1) } else { a };
            out.push(a + (b - a) * frac);
        }
    } else {
        for i in 0..out_len {
            let pos = i as f64 * ratio;
            let i0 = pos.floor() as usize;
            let frac = (pos - i0 as f64) as f32;
            let a = input[i0];
            let b = *input.get(i0 + 1).unwrap_or(&a);
            out.push(a + (b - a) * frac);
        }
    }
    out
}

pub fn to_i16(x: &[f32]) -> Vec<i16> {
    x.iter().map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16).collect()
}

/// Cheap speech gate: does any 30 ms window rise above `threshold_dbfs` RMS,
/// and is the clip at least `min_ms` long? Prevents sending silence to the STT
/// model, which tends to hallucinate ("Thank you.") on empty audio.
pub fn has_speech(pcm16k: &[f32], threshold_dbfs: f32, min_ms: u32) -> bool {
    let min_len = (TARGET_RATE as usize * min_ms as usize) / 1000;
    if pcm16k.len() < min_len {
        return false;
    }
    let win = (TARGET_RATE as usize * 30) / 1000;
    let thr = 10f32.powf(threshold_dbfs / 20.0);
    pcm16k.chunks(win).any(|c| {
        let rms = (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt();
        rms >= thr
    })
}

pub fn duration_ms(samples_16k: usize) -> u64 {
    (samples_16k as u64 * 1000) / TARGET_RATE as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: u32, secs: f32, amp: f32) -> Vec<f32> {
        let n = (rate as f32 * secs) as usize;
        (0..n).map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin()).collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
    }

    #[test]
    fn length_is_correct() {
        let x = vec![0.0; 48_000];
        assert_eq!(to_16k(&x, 48_000).len(), 16_000);
        let x = vec![0.0; 44_100];
        assert_eq!(to_16k(&x, 44_100).len(), 16_000);
        let x = vec![0.0; 8_000];
        assert_eq!(to_16k(&x, 8_000).len(), 16_000);
    }

    #[test]
    fn passband_preserved() {
        // 1 kHz tone survives 48k → 16k at ~same level
        let y = to_16k(&sine(1000.0, 48_000, 1.0, 0.5), 48_000);
        let r = rms(&y[200..y.len() - 200]);
        assert!((r - 0.5 / 2f32.sqrt()).abs() < 0.02, "rms {r}");
    }

    #[test]
    fn alias_rejected() {
        // 12 kHz tone would alias to 4 kHz without filtering; must be strongly attenuated
        let y = to_16k(&sine(12_000.0, 48_000, 1.0, 0.5), 48_000);
        let r = rms(&y[200..y.len() - 200]);
        assert!(r < 0.01, "alias rms {r}");
    }

    #[test]
    fn i16_conversion_clamps() {
        assert_eq!(to_i16(&[0.0, 1.0, -1.0, 2.0]), vec![0, 32767, -32767, 32767]);
    }

    #[test]
    fn speech_gate() {
        assert!(!has_speech(&vec![0.0; 16_000], -45.0, 250), "silence");
        assert!(!has_speech(&sine(300.0, 16_000, 0.1, 0.3), -45.0, 250), "too short");
        assert!(has_speech(&sine(300.0, 16_000, 1.0, 0.05), -45.0, 250), "quiet speech-level tone");
        assert!(!has_speech(&sine(300.0, 16_000, 1.0, 0.001), -45.0, 250), "room noise");
    }
}
