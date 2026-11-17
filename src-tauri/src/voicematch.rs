//! Voice match: recognises *your* "Hey Ledge" from samples of your voice.
//!
//! Windows' recogniser is generic; quick or accented "Hey Ledge" often comes
//! out as "hey edge" or just "hey". This listens to the microphone itself
//! (16 kHz, on this PC only), turns sound into MFCC features and, ten times a
//! second, compares the last 1.2 s with your samples using dynamic time
//! warping. A close match wakes Ledge directly; a weaker one vouches for what
//! Windows heard. Samples live in %APPDATA%\Ledge\voice\*.wav; Ledge adds one
//! whenever Windows clearly hears "Hey Ledge" but the match missed it, so it
//! keeps learning your voice. Nothing is uploaded.

use crate::config;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const SR: usize = 16_000;
const HOP: usize = 160; // 10 ms
const WIN: usize = 400; // 25 ms
const NFFT: usize = 512;
const NMEL: usize = 20;
const NCEP: usize = 12;
const WINDOW_FRAMES: usize = 120; // 1.2 s compared each check
const MAX_AUTO: usize = 9;

pub type Frame = [f64; NCEP];

// ---------- features ----------

struct Dsp {
    hamming: Vec<f64>,
    mel: Vec<Vec<(usize, f64)>>,
    dct: Vec<[f64; NMEL]>,
}

fn dsp() -> &'static Dsp {
    static D: std::sync::OnceLock<Dsp> = std::sync::OnceLock::new();
    D.get_or_init(|| {
        let hamming = (0..WIN).map(|n| 0.54 - 0.46 * (2.0 * std::f64::consts::PI * n as f64 / (WIN - 1) as f64).cos()).collect();
        let m = |h: f64| 2595.0 * (1.0 + h / 700.0).log10();
        let im = |v: f64| 700.0 * (10f64.powf(v / 2595.0) - 1.0);
        let (lo, hi) = (m(100.0), m(4000.0));
        let bins: Vec<usize> =
            (0..NMEL + 2).map(|i| (((NFFT + 1) as f64) * im(lo + (hi - lo) * i as f64 / (NMEL + 1) as f64) / SR as f64).floor() as usize).collect();
        let mel = (1..=NMEL)
            .map(|i| {
                let (l, c, r) = (bins[i - 1], bins[i], bins[i + 1]);
                let mut f = Vec::new();
                for k in l..c {
                    f.push((k, (k - l) as f64 / (c - l).max(1) as f64));
                }
                for k in c..r {
                    f.push((k, (r - k) as f64 / (r - c).max(1) as f64));
                }
                f
            })
            .collect();
        let dct = (1..=NCEP)
            .map(|k| {
                let mut row = [0.0; NMEL];
                for (n, v) in row.iter_mut().enumerate() {
                    *v = (std::f64::consts::PI * k as f64 * (n as f64 + 0.5) / NMEL as f64).cos();
                }
                row
            })
            .collect();
        Dsp { hamming, mel, dct }
    })
}

/// In-place radix-2 FFT (re, im), n a power of two.
fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f64::consts::PI / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (ang * k as f64).sin_cos();
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * c - im[b] * s, re[b] * s + im[b] * c);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

/// MFCC (c1..c12) and log energy for one 25 ms frame of pre-emphasised samples.
fn frame_features(x: &[f64]) -> (Frame, f64) {
    let d = dsp();
    let mut re = vec![0.0; NFFT];
    let mut im = vec![0.0; NFFT];
    for i in 0..WIN {
        re[i] = x[i] * d.hamming[i];
    }
    fft(&mut re, &mut im);
    let pow: Vec<f64> = (0..=NFFT / 2).map(|k| re[k] * re[k] + im[k] * im[k]).collect();
    let mel: Vec<f64> = d.mel.iter().map(|f| (f.iter().map(|&(k, w)| w * pow[k]).sum::<f64>() + 2e4).ln()).collect();
    let mut out = [0.0; NCEP];
    for (k, row) in d.dct.iter().enumerate() {
        out[k] = row.iter().zip(&mel).map(|(a, b)| a * b).sum();
    }
    (out, (pow.iter().sum::<f64>() + 1.0).ln())
}

/// Features for a whole signal (samples on the 16-bit scale).
pub fn features(x: &[f64]) -> (Vec<Frame>, Vec<f64>) {
    if x.len() < WIN {
        return (Vec::new(), Vec::new());
    }
    let mut p = Vec::with_capacity(x.len());
    p.push(x[0]);
    for i in 1..x.len() {
        p.push(x[i] - 0.97 * x[i - 1]);
    }
    (0..=(p.len() - WIN) / HOP).map(|f| frame_features(&p[f * HOP..f * HOP + WIN])).unzip()
}

fn percentile(v: &[f64], q: f64) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    s.get(((s.len() as f64 - 1.0) * q) as usize).copied().unwrap_or(0.0)
}

/// Voiced frames: about 9 dB over the noise floor.
fn voiced(e: &[f64], floor: f64) -> Vec<bool> {
    e.iter().map(|&v| v > floor + 2.0).collect()
}

fn mean_normalise(f: &[Frame], mask: &[bool]) -> Vec<Frame> {
    let n = mask.iter().filter(|&&v| v).count().max(1) as f64;
    let mut mu = [0.0; NCEP];
    for (fr, &m) in f.iter().zip(mask) {
        if m {
            for k in 0..NCEP {
                mu[k] += fr[k] / n;
            }
        }
    }
    f.iter().map(|fr| std::array::from_fn(|k| fr[k] - mu[k])).collect()
}

/// Deterministic low-level noise, like a real microphone's floor.
fn with_noise(x: &[f64], lead: f64, tail: f64) -> Vec<f64> {
    let mut s = vec![0.0; (lead * SR as f64) as usize];
    s.extend_from_slice(x);
    s.extend(std::iter::repeat_n(0.0, (tail * SR as f64) as usize));
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut rnd = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    for v in &mut s {
        let (u1, u2) = (rnd().max(1e-12), rnd());
        *v += 8.0 * (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
    }
    s
}

/// A template from one clip of you saying the wake phrase: voiced part only, mean-normalised.
pub fn template(clip: &[f64]) -> Option<Vec<Frame>> {
    let (f, e) = features(&with_noise(clip, 0.2, 0.2));
    let v = voiced(&e, percentile(&e, 0.1));
    let first = v.iter().position(|&b| b)?;
    let last = v.iter().rposition(|&b| b)?;
    (last - first >= 25).then(|| mean_normalise(&f[first..=last], &v[first..=last]))
}

/// Template vs window: the match may start anywhere and must end at the window's
/// last frame. Mean frame distance along the path; slope limited (no two
/// non-diagonal steps in a row).
pub fn dtw_end(t: &[Frame], q: &[Frame]) -> f64 {
    let (tn, qn) = (t.len(), q.len());
    if tn < 2 || qn < 2 {
        return f64::INFINITY;
    }
    let dist = |i: usize, j: usize| t[i].iter().zip(&q[j]).map(|(a, b)| (a - b) * (a - b)).sum::<f64>().sqrt();
    // (cost, length) for arrival by a diagonal step [0] or another step [1]
    let mut prev = vec![[(f64::INFINITY, 1.0); 2]; qn];
    for (j, p) in prev.iter_mut().enumerate() {
        p[0] = (dist(0, j), 1.0);
    }
    let mean = |(c, l): (f64, f64)| c / l;
    for i in 1..tn {
        let mut cur = vec![[(f64::INFINITY, 1.0); 2]; qn];
        for j in 1..qn {
            let c = dist(i, j);
            let d = if mean(prev[j - 1][0]) <= mean(prev[j - 1][1]) { prev[j - 1][0] } else { prev[j - 1][1] };
            cur[j][0] = (d.0 + c, d.1 + 1.0);
            let (v, h) = (prev[j][0], cur[j - 1][0]);
            let o = if mean(v) <= mean(h) { v } else { h };
            cur[j][1] = (o.0 + c, o.1 + 1.0);
        }
        prev = cur;
    }
    mean(prev[qn - 1][0]).min(mean(prev[qn - 1][1]))
}

/// The best score of `templates` against windows ending every 100 ms in a stream.
pub fn best_in_stream(templates: &[Vec<Frame>], x: &[f64]) -> f64 {
    let (f, e) = features(&with_noise(x, 0.5, 0.6));
    let v = voiced(&e, percentile(&e, 0.1));
    let mut best = f64::INFINITY;
    let mut end = 40;
    while end <= f.len() {
        best = best.min(score_window(templates, &f, &v, end));
        end += 10;
    }
    best
}

/// Score for the window ending at `end` (exclusive), or infinity if it isn't speech ending now.
fn score_window(templates: &[Vec<Frame>], f: &[Frame], v: &[bool], end: usize) -> f64 {
    let a = end.saturating_sub(WINDOW_FRAMES);
    if v[a..end].iter().filter(|&&b| b).count() < 25 || !v[end.saturating_sub(15)..end].iter().any(|&b| b) {
        return f64::INFINITY;
    }
    let w = mean_normalise(&f[a..end], &v[a..end]);
    templates.iter().map(|t| dtw_end(t, &w)).fold(f64::INFINITY, f64::min)
}

/// Thresholds from your own samples (each scored against the others):
/// strong = wakes Ledge on its own; weak = vouches for Windows' guess.
pub fn calibrate(clips: &[Vec<f64>]) -> Option<(f64, f64)> {
    if clips.len() < 3 {
        return None;
    }
    let temps: Vec<Option<Vec<Frame>>> = clips.iter().map(|c| template(c)).collect();
    let mut own: Vec<f64> = (0..clips.len())
        .filter_map(|i| {
            let others: Vec<Vec<Frame>> = temps.iter().enumerate().filter(|(k, t)| *k != i && t.is_some()).map(|(_, t)| t.clone().unwrap()).collect();
            let s = best_in_stream(&others, &clips[i]);
            s.is_finite().then_some(s)
        })
        .collect();
    if own.len() < 3 {
        return None;
    }
    own.sort_by(|a, b| a.total_cmp(b));
    let median = own[own.len() / 2];
    let worst = *own.last().unwrap();
    let strong = median + 0.6;
    // Background sound scored ~15.9-16.2 live, so vouching stays close to the wake line.
    Some((strong, (worst + 0.4).min(strong + 0.8)))
}

// ---------- samples on disk ----------

pub fn dir() -> PathBuf {
    config::dir().join("voice")
}

pub fn read_wav(p: &Path) -> Option<Vec<f64>> {
    let b = std::fs::read(p).ok()?;
    if b.len() < 44 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return None;
    }
    let (mut i, mut ch, mut rate, mut bits) = (12, 1u16, 0u32, 0u16);
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let len = u32::from_le_bytes(b[i + 4..i + 8].try_into().ok()?) as usize;
        let body = &b[i + 8..(i + 8 + len).min(b.len())];
        if id == b"fmt " && body.len() >= 16 {
            ch = u16::from_le_bytes([body[2], body[3]]);
            rate = u32::from_le_bytes(body[4..8].try_into().ok()?);
            bits = u16::from_le_bytes([body[14], body[15]]);
        } else if id == b"data" {
            if rate as usize != SR || bits != 16 || ch == 0 {
                return None;
            }
            let frames = body.len() / (2 * ch as usize);
            return Some(
                (0..frames)
                    .map(|f| (0..ch as usize).map(|c| i16::from_le_bytes([body[(f * ch as usize + c) * 2], body[(f * ch as usize + c) * 2 + 1]]) as f64).sum::<f64>() / ch as f64)
                    .collect(),
            );
        }
        i += 8 + len + (len & 1);
    }
    None
}

pub fn write_wav(p: &Path, x: &[f64]) -> std::io::Result<()> {
    let data: Vec<u8> = x.iter().flat_map(|v| (v.round().clamp(-32768.0, 32767.0) as i16).to_le_bytes()).collect();
    let mut b = Vec::with_capacity(44 + data.len());
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&(SR as u32).to_le_bytes());
    b.extend_from_slice(&(SR as u32 * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(data.len() as u32).to_le_bytes());
    b.extend_from_slice(&data);
    std::fs::write(p, b)
}

struct Model {
    templates: Vec<Vec<Frame>>,
    strong: f64,
    weak: f64,
}

static MODEL: Mutex<Option<Model>> = Mutex::new(None);

fn sample_files() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir())
        .map(|r| r.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("wav"))).collect())
        .unwrap_or_default();
    v.sort();
    v
}

/// (Re)builds the model from the samples folder. Returns how many samples it uses.
pub fn reload() -> usize {
    let clips: Vec<Vec<f64>> = sample_files().iter().filter_map(|p| read_wav(p)).collect();
    let templates: Vec<Vec<Frame>> = clips.iter().filter_map(|c| template(c)).collect();
    let model = calibrate(&clips).map(|(strong, weak)| Model { templates: templates.clone(), strong, weak });
    let n = templates.len();
    match &model {
        Some(m) => log(&format!("voice match ready: {n} samples, wake under {:.2}, vouch under {:.2}", m.strong, m.weak)),
        None => log(&format!("voice match off: needs 3 clear samples in {} (has {n})", dir().display())),
    }
    *MODEL.lock().unwrap_or_else(|e| e.into_inner()) = model;
    n
}

pub fn sample_count() -> usize {
    sample_files().len()
}

fn log(s: &str) {
    crate::voice::log_line(s);
}

// ---------- live listening ----------

static RUNNING: AtomicBool = AtomicBool::new(false);
static WANTED: AtomicBool = AtomicBool::new(false);
/// The last few seconds of microphone audio (for learning new samples).
static RECENT: Mutex<Vec<f64>> = Mutex::new(Vec::new());

/// Starts listening (if not already). `on_match(strong, score)` runs on each match.
pub fn start(on_match: impl Fn(bool, f64) + Send + 'static) {
    WANTED.store(true, Ordering::SeqCst);
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        if MODEL.lock().unwrap_or_else(|e| e.into_inner()).is_none() {
            reload();
        }
        while WANTED.load(Ordering::SeqCst) {
            if let Err(e) = capture(&on_match) {
                log(&format!("voice match: microphone stopped ({e}); retrying"));
            }
            std::thread::sleep(Duration::from_secs(2));
        }
        RUNNING.store(false, Ordering::SeqCst);
    });
}

pub fn stop() {
    WANTED.store(false, Ordering::SeqCst);
}

fn capture(on_match: &impl Fn(bool, f64)) -> windows::core::Result<()> {
    use windows::Win32::Media::Audio::{
        AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, IAudioCaptureClient,
        IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX, eCapture, eConsole,
    };
    use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
    unsafe {
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let dev = en.GetDefaultAudioEndpoint(eCapture, eConsole)?;
        let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
        let fmt = WAVEFORMATEX { wFormatTag: 1, nChannels: 1, nSamplesPerSec: SR as u32, nAvgBytesPerSec: SR as u32 * 2, nBlockAlign: 2, wBitsPerSample: 16, cbSize: 0 };
        client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, 2_000_000, 0, &fmt, None)?;
        let cap: IAudioCaptureClient = client.GetService()?;
        client.Start()?;
        let _stop = scopeguard(|| {
            let _ = client.Stop();
        });

        let mut pending: Vec<f64> = Vec::new(); // pre-emphasised samples not yet framed
        let mut last_raw = 0.0;
        let mut feats: Vec<Frame> = Vec::new();
        let mut energy: Vec<f64> = Vec::new();
        let mut since_check = 0usize;
        let mut cooldown_until = Instant::now();
        let mut rescan = Instant::now();
        while WANTED.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(30));
            // Pick up a newly chosen default microphone.
            if rescan.elapsed() > Duration::from_secs(10) {
                rescan = Instant::now();
                if let Ok(d) = en.GetDefaultAudioEndpoint(eCapture, eConsole) {
                    if d.GetId().ok().map(|p| p.to_string().unwrap_or_default()) != dev.GetId().ok().map(|p| p.to_string().unwrap_or_default()) {
                        return Ok(());
                    }
                }
            }
            let mut fresh: Vec<f64> = Vec::new();
            loop {
                let n = cap.GetNextPacketSize()?;
                if n == 0 {
                    break;
                }
                let mut data = std::ptr::null_mut();
                let (mut frames, mut flags) = (0u32, 0u32);
                cap.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                    fresh.extend(std::iter::repeat_n(0.0, frames as usize));
                } else {
                    let s = std::slice::from_raw_parts(data as *const i16, frames as usize);
                    fresh.extend(s.iter().map(|&v| v as f64));
                }
                cap.ReleaseBuffer(frames)?;
            }
            if fresh.is_empty() {
                continue;
            }
            {
                let mut r = RECENT.lock().unwrap_or_else(|e| e.into_inner());
                r.extend_from_slice(&fresh);
                let keep = 3 * SR;
                if r.len() > keep {
                    let cut = r.len() - keep;
                    r.drain(..cut);
                }
            }
            for v in fresh {
                pending.push(v - 0.97 * last_raw);
                last_raw = v;
            }
            while pending.len() >= WIN {
                let (f, e) = frame_features(&pending[..WIN]);
                feats.push(f);
                energy.push(e);
                pending.drain(..HOP);
                since_check += 1;
            }
            let keep = 300; // 3 s of frames, for the noise floor
            if feats.len() > keep {
                let cut = feats.len() - keep;
                feats.drain(..cut);
                energy.drain(..cut);
            }
            if since_check < 10 || feats.len() < 40 || Instant::now() < cooldown_until {
                continue;
            }
            since_check = 0;
            let model = MODEL.lock().unwrap_or_else(|e| e.into_inner());
            let Some(m) = model.as_ref() else { continue };
            let v = voiced(&energy, percentile(&energy, 0.1));
            let s = score_window(&m.templates, &feats, &v, feats.len());
            if s < m.weak {
                let strong = s < m.strong;
                drop(model);
                cooldown_until = Instant::now() + Duration::from_millis(if strong { 2000 } else { 600 });
                on_match(strong, s);
            }
        }
    }
    Ok(())
}

struct Guard<F: FnMut()>(F);
impl<F: FnMut()> Drop for Guard<F> {
    fn drop(&mut self) {
        (self.0)();
    }
}
fn scopeguard<F: FnMut()>(f: F) -> Guard<F> {
    Guard(f)
}

/// Saves the last thing you said (if it looks like a short phrase) as a new
/// sample, keeping at most MAX_AUTO learned ones, and rebuilds the model.
pub fn learn_last() -> bool {
    let audio = RECENT.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if audio.len() < SR {
        return false;
    }
    let (_, e) = features(&audio);
    let v = voiced(&e, percentile(&e, 0.1));
    // the last voiced run, bridging gaps under 250 ms
    let Some(end) = v.iter().rposition(|&b| b) else { return false };
    let mut start = end;
    let mut gap = 0;
    for i in (0..end).rev() {
        if v[i] {
            start = i;
            gap = 0;
        } else {
            gap += 1;
            if gap > 25 {
                break;
            }
        }
    }
    let frames = end - start;
    if !(30..=130).contains(&frames) {
        return false;
    }
    let a = (start * HOP).saturating_sub(SR / 20);
    let b = ((end * HOP) + WIN + SR / 20).min(audio.len());
    let _ = std::fs::create_dir_all(dir());
    let name = format!("learned-{}.wav", chrono::Local::now().format("%Y%m%d-%H%M%S"));
    if write_wav(&dir().join(&name), &audio[a..b]).is_err() {
        return false;
    }
    let mut learned: Vec<PathBuf> = sample_files().into_iter().filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("learned-"))).collect();
    while learned.len() > MAX_AUTO {
        let _ = std::fs::remove_file(learned.remove(0));
    }
    log(&format!("voice match learned a new sample ({name})"));
    reload();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f64, secs: f64) -> Vec<f64> {
        (0..(secs * SR as f64) as usize).map(|i| 3000.0 * (2.0 * std::f64::consts::PI * freq * i as f64 / SR as f64).sin()).collect()
    }

    #[test]
    fn fft_matches_a_known_tone() {
        let mut re: Vec<f64> = (0..NFFT).map(|i| (2.0 * std::f64::consts::PI * 8.0 * i as f64 / NFFT as f64).cos()).collect();
        let mut im = vec![0.0; NFFT];
        fft(&mut re, &mut im);
        let peak = (0..NFFT / 2).max_by(|&a, &b| (re[a].hypot(im[a])).total_cmp(&re[b].hypot(im[b]))).unwrap();
        assert_eq!(peak, 8);
    }

    #[test]
    fn wav_round_trips() {
        let p = std::env::temp_dir().join("ledge-voicematch-test.wav");
        let x = tone(440.0, 0.1);
        write_wav(&p, &x).unwrap();
        let y = read_wav(&p).unwrap();
        assert_eq!(x.len(), y.len());
        assert!((x[100] - y[100]).abs() < 1.0);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn same_sound_matches_closer_than_a_different_one() {
        // a rising then falling sweep vs a flat tone
        let sweep: Vec<f64> = (0..SR * 6 / 10)
            .map(|i| {
                let t = i as f64 / SR as f64;
                let f = 300.0 + 1500.0 * (std::f64::consts::PI * t / 0.6).sin();
                3000.0 * (2.0 * std::f64::consts::PI * f * t).sin()
            })
            .collect();
        let t = template(&sweep).expect("template");
        assert!(best_in_stream(&[t.clone()], &sweep) < best_in_stream(&[t], &tone(700.0, 0.6)));
    }

    /// Scores the samples in %APPDATA%\Ledge\voice and any WAVs in LEDGE_VOICE_NEG.
    /// `cargo test --release --lib voicematch::tests::live_samples -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_samples() {
        let files = sample_files();
        let clips: Vec<Vec<f64>> = files.iter().filter_map(|p| read_wav(p)).collect();
        println!("{} samples", clips.len());
        let (strong, weak) = calibrate(&clips).expect("needs 3 samples");
        println!("wake under {strong:.2}, vouch under {weak:.2}");
        let temps: Vec<Vec<Frame>> = clips.iter().filter_map(|c| template(c)).collect();
        if let Ok(d) = std::env::var("LEDGE_VOICE_NEG") {
            let mut worst = f64::INFINITY;
            let mut false_wakes = 0;
            for e in std::fs::read_dir(d).unwrap().flatten() {
                if let Some(x) = read_wav(&e.path()) {
                    let s = best_in_stream(&temps, &x);
                    worst = worst.min(s);
                    if s < strong {
                        false_wakes += 1;
                        println!("  would wake: {} {s:.2}", e.path().display());
                    }
                }
            }
            println!("closest other speech {worst:.2}; false wakes {false_wakes}");
        }
    }
}
