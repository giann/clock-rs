use std::{
    io::Read,
    process::{Command, Output, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
};

use crate::config::VisualizerConfig;

#[derive(Clone, Copy, Default)]
struct MeterValues {
    left_level: f32,
    right_level: f32,
    left_brightness: f32,
    right_brightness: f32,
}

struct MeterHandle {
    values: Arc<Mutex<MeterValues>>,
    stop: Arc<AtomicBool>,
}

impl MeterHandle {
    fn spawn(device_name: String) -> Option<Self> {
        let values = Arc::new(Mutex::new(MeterValues::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_values = Arc::clone(&values);
        let thread_stop = Arc::clone(&stop);
        let selected_device = resolve_audio_device_name(&device_name).unwrap_or(device_name);

        thread::spawn(move || {
            let _ = run_meter_thread(thread_values, thread_stop, selected_device);
        });

        Some(Self { values, stop })
    }

    fn snapshot(&self) -> MeterValues {
        self.values.lock().map(|v| *v).unwrap_or_default()
    }
}

impl Drop for MeterHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

pub struct Visualizer {
    columns_per_side: usize,
    meter: Option<MeterHandle>,
    plain_line: Option<String>,
    styled_line: Option<String>,
}

impl Visualizer {
    pub fn from_config(config: VisualizerConfig) -> Option<Self> {
        if !config.enabled {
            return None;
        }

        let columns_per_side = config.columns_per_side.clamp(6, 30);

        Some(Self {
            columns_per_side,
            meter: MeterHandle::spawn(
                config
                    .input_device
                    .unwrap_or_else(|| "BlackHole 2ch".to_string()),
            ),
            plain_line: None,
            styled_line: None,
        })
    }

    pub fn update(&mut self) {
        let Some(meter) = &self.meter else {
            self.plain_line = None;
            self.styled_line = None;
            return;
        };

        let snapshot = meter.snapshot();
        let (plain, styled) = Self::build_line(
            self.columns_per_side,
            snapshot.left_level,
            snapshot.right_level,
            snapshot.left_brightness,
            snapshot.right_brightness,
        );
        self.plain_line = Some(plain);
        self.styled_line = Some(styled);
    }

    pub fn plain_line(&self) -> Option<&str> {
        self.plain_line.as_deref()
    }

    pub fn styled_line(&self) -> Option<&str> {
        self.styled_line.as_deref()
    }

    fn build_line(
        columns: usize,
        left_level: f32,
        right_level: f32,
        left_brightness: f32,
        right_brightness: f32,
    ) -> (String, String) {
        let left_inner = Self::side_from_center_out(left_level, columns);
        let right = Self::side_from_center_out(right_level, columns);
        let left: String = left_inner.chars().rev().collect();
        let plain = format!("{left}│{right}");

        let (left_fg, left_bg) = Self::colors_from_brightness(left_brightness);
        let (right_fg, right_bg) = Self::colors_from_brightness(right_brightness);

        let styled = format!(
            "{}{}{}{}\u{2502}{}{}{}{}",
            Self::fg_escape(left_fg),
            Self::bg_escape(left_bg),
            left,
            crate::color::Color::RESET,
            Self::fg_escape(right_fg),
            Self::bg_escape(right_bg),
            right,
            crate::color::Color::RESET
        );

        (plain, styled)
    }

    fn side_from_center_out(level: f32, columns: usize) -> String {
        let filled = (level.clamp(0.0, 1.0) * columns as f32).round() as usize;
        let mut chars = Vec::with_capacity(columns);

        for i in 0..columns {
            if i >= filled {
                chars.push(' ');
                continue;
            }

            let distance = i as f32 / (columns.saturating_sub(1).max(1)) as f32;
            let ch = if distance < 0.25 {
                '█'
            } else if distance < 0.5 {
                '▆'
            } else if distance < 0.75 {
                '▄'
            } else {
                '▂'
            };

            chars.push(ch);
        }

        chars.iter().collect()
    }

    fn colors_from_brightness(brightness: f32) -> ((u8, u8, u8), (u8, u8, u8)) {
        let brightness = brightness.clamp(0.0, 1.0);
        let hue = 220.0 - 200.0 * brightness as f64;
        let fg = Self::hsv_to_rgb(hue, 0.9, 0.95);
        let bg = (
            ((fg.0 as f32) * 0.25) as u8,
            ((fg.1 as f32) * 0.25) as u8,
            ((fg.2 as f32) * 0.25) as u8,
        );

        (fg, bg)
    }

    fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (u8, u8, u8) {
        let c = v * s;
        let hh = (h / 60.0) % 6.0;
        let x = c * (1.0 - ((hh % 2.0) - 1.0).abs());

        let (r1, g1, b1) = if (0.0..1.0).contains(&hh) {
            (c, x, 0.0)
        } else if (1.0..2.0).contains(&hh) {
            (x, c, 0.0)
        } else if (2.0..3.0).contains(&hh) {
            (0.0, c, x)
        } else if (3.0..4.0).contains(&hh) {
            (0.0, x, c)
        } else if (4.0..5.0).contains(&hh) {
            (x, 0.0, c)
        } else {
            (c, 0.0, x)
        };

        let m = v - c;
        let r = ((r1 + m) * 255.0).round().clamp(0.0, 255.0) as u8;
        let g = ((g1 + m) * 255.0).round().clamp(0.0, 255.0) as u8;
        let b = ((b1 + m) * 255.0).round().clamp(0.0, 255.0) as u8;

        (r, g, b)
    }

    fn fg_escape((r, g, b): (u8, u8, u8)) -> String {
        format!("\x1B[38;2;{r};{g};{b}m")
    }

    fn bg_escape((r, g, b): (u8, u8, u8)) -> String {
        format!("\x1B[48;2;{r};{g};{b}m")
    }
}

#[derive(Default)]
struct MeterAccumulator {
    frames: usize,
    left_sum_sq: f64,
    right_sum_sq: f64,
    left_sum_abs: f64,
    right_sum_abs: f64,
    left_sum_diff_abs: f64,
    right_sum_diff_abs: f64,
    prev_left: f32,
    prev_right: f32,
    smooth_left_level: f32,
    smooth_right_level: f32,
    smooth_left_brightness: f32,
    smooth_right_brightness: f32,
}

impl MeterAccumulator {
    const WINDOW_FRAMES: usize = 1024;
    const SMOOTHING: f32 = 0.22;

    fn push_frame(&mut self, left: f32, right: f32, shared: &Arc<Mutex<MeterValues>>) {
        let left = left.clamp(-1.0, 1.0);
        let right = right.clamp(-1.0, 1.0);

        self.frames += 1;
        self.left_sum_sq += (left as f64) * (left as f64);
        self.right_sum_sq += (right as f64) * (right as f64);
        self.left_sum_abs += (left.abs()) as f64;
        self.right_sum_abs += (right.abs()) as f64;
        self.left_sum_diff_abs += (left - self.prev_left).abs() as f64;
        self.right_sum_diff_abs += (right - self.prev_right).abs() as f64;
        self.prev_left = left;
        self.prev_right = right;

        if self.frames < Self::WINDOW_FRAMES {
            return;
        }

        let frames = self.frames as f64;
        let left_rms = (self.left_sum_sq / frames).sqrt() as f32;
        let right_rms = (self.right_sum_sq / frames).sqrt() as f32;
        let left_level = Self::rms_to_level(left_rms);
        let right_level = Self::rms_to_level(right_rms);

        let left_brightness = (self.left_sum_diff_abs / (self.left_sum_abs.max(1e-6))) as f32;
        let right_brightness = (self.right_sum_diff_abs / (self.right_sum_abs.max(1e-6))) as f32;

        self.smooth_left_level =
            self.smooth_left_level * (1.0 - Self::SMOOTHING) + left_level * Self::SMOOTHING;
        self.smooth_right_level =
            self.smooth_right_level * (1.0 - Self::SMOOTHING) + right_level * Self::SMOOTHING;
        self.smooth_left_brightness = self.smooth_left_brightness * (1.0 - Self::SMOOTHING)
            + (left_brightness / 1.4).clamp(0.0, 1.0) * Self::SMOOTHING;
        self.smooth_right_brightness = self.smooth_right_brightness * (1.0 - Self::SMOOTHING)
            + (right_brightness / 1.4).clamp(0.0, 1.0) * Self::SMOOTHING;

        if let Ok(mut meter) = shared.lock() {
            meter.left_level = self.smooth_left_level;
            meter.right_level = self.smooth_right_level;
            meter.left_brightness = self.smooth_left_brightness;
            meter.right_brightness = self.smooth_right_brightness;
        }

        self.frames = 0;
        self.left_sum_sq = 0.0;
        self.right_sum_sq = 0.0;
        self.left_sum_abs = 0.0;
        self.right_sum_abs = 0.0;
        self.left_sum_diff_abs = 0.0;
        self.right_sum_diff_abs = 0.0;
    }

    fn rms_to_level(rms: f32) -> f32 {
        // Convert RMS to a dB-ish perceptual scale to avoid a near-flat visualizer at low levels.
        let db = 20.0 * rms.max(1e-6).log10();
        ((db + 48.0) / 48.0).clamp(0.0, 1.0)
    }
}

fn resolve_audio_device_name(device_hint: &str) -> Option<String> {
    let hint = device_hint.trim();
    if hint.is_empty() {
        return None;
    }

    let output = list_avfoundation_devices()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let devices = parse_avfoundation_audio_devices(&stderr);
    if devices.is_empty() {
        return None;
    }

    let hint_lower = hint.to_ascii_lowercase();

    devices
        .iter()
        .find(|name| name.eq_ignore_ascii_case(hint))
        .cloned()
        .or_else(|| {
            devices
                .iter()
                .find(|name| name.to_ascii_lowercase().contains(&hint_lower))
                .cloned()
        })
        .or_else(|| {
            if hint_lower.contains("multi-output") {
                devices
                    .iter()
                    .find(|name| name.to_ascii_lowercase().contains("blackhole"))
                    .cloned()
            } else {
                None
            }
        })
}

fn list_avfoundation_devices() -> Option<Output> {
    Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-f")
        .arg("avfoundation")
        .arg("-list_devices")
        .arg("true")
        .arg("-i")
        .arg("")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .output()
        .ok()
}

fn parse_avfoundation_audio_devices(stderr: &str) -> Vec<String> {
    let mut devices = Vec::new();
    let mut in_audio_section = false;

    for line in stderr.lines() {
        if line.contains("AVFoundation audio devices") {
            in_audio_section = true;
            continue;
        }

        if line.contains("AVFoundation video devices") {
            in_audio_section = false;
            continue;
        }

        if !in_audio_section {
            continue;
        }

        let Some((_, name)) = line.rsplit_once("] ") else {
            continue;
        };
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            devices.push(trimmed.to_string());
        }
    }

    devices
}

fn run_meter_thread(
    shared: Arc<Mutex<MeterValues>>,
    stop: Arc<AtomicBool>,
    device_name: String,
) -> Result<(), String> {
    let mut child = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-f")
        .arg("avfoundation")
        .arg("-i")
        .arg(format!(":{device_name}"))
        .arg("-ac")
        .arg("2")
        .arg("-ar")
        .arg("22050")
        .arg("-f")
        .arg("f32le")
        .arg("-")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|err| err.to_string())?;

    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill();
        return Err("failed to capture ffmpeg stdout".to_string());
    };

    let mut acc = MeterAccumulator::default();
    let mut buffer = vec![0u8; 8 * 512];

    while !stop.load(Ordering::Relaxed) {
        match stdout.read_exact(&mut buffer) {
            Ok(()) => {
                for frame in buffer.chunks_exact(8) {
                    let left = f32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]);
                    let right = f32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
                    acc.push_frame(left, right, &shared);
                }
            }
            Err(_) => break,
        }
    }

    let _ = child.kill();
    let _ = child.wait();

    Ok(())
}
