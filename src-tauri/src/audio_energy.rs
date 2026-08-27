use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::{
    analysis_cancel::{self, CommandWaitError},
    render::{configure_background_process, resolve_ffmpeg_path, RenderErrorReport},
};

const ENGINE_NAME: &str = "ffmpeg-ebur128-momentary-v1";
const MAX_DURATION_MS: u64 = 4 * 60 * 60 * 1_000;
const BUCKET_MS: u64 = 250;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeAudioEnergyRequest {
    pub source_path: String,
    #[serde(default)]
    pub start_ms: u64,
    pub duration_ms: u64,
    #[serde(default = "default_playback_rate")]
    pub playback_rate: f64,
    #[serde(default)]
    pub ffmpeg_path: Option<String>,
    #[serde(default)]
    pub operation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioEnergyPoint {
    pub at_ms: u64,
    pub loudness_db: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeAudioEnergyResponse {
    pub engine: &'static str,
    pub duration_ms: u64,
    pub points: Vec<AudioEnergyPoint>,
}

#[tauri::command]
pub async fn analyze_audio_energy(
    app: AppHandle,
    request: AnalyzeAudioEnergyRequest,
) -> Result<AnalyzeAudioEnergyResponse, RenderErrorReport> {
    if analysis_cancel::is_cancelled(request.operation_id.as_deref()) {
        return Err(audio_energy_error(
            "smart_shorts_cancelled",
            "Akıllı Shorts analizi iptal edildi.",
            "Audio energy analysis cancelled before FFmpeg start",
            false,
        ));
    }
    validate_request(&request)?;
    let source_path = validate_source_path(&request.source_path)?;
    let ffmpeg_path = resolve_ffmpeg_path(&app, request.ffmpeg_path.as_deref());
    tauri::async_runtime::spawn_blocking(move || {
        analyze_audio_energy_blocking(&ffmpeg_path, &source_path, &request)
    })
    .await
    .map_err(|error| {
        audio_energy_error(
            "audio_energy_worker_failed",
            "Ses enerjisi analizi tamamlanamadı.",
            format!("Audio energy worker failed: {error}"),
            true,
        )
    })?
}

fn analyze_audio_energy_blocking(
    ffmpeg_path: &Path,
    source_path: &Path,
    request: &AnalyzeAudioEnergyRequest,
) -> Result<AnalyzeAudioEnergyResponse, RenderErrorReport> {
    let expected_duration_ms = post_speed_duration_ms(request.duration_ms, request.playback_rate);
    let mut filters = atempo_filters(request.playback_rate);
    filters.push("ebur128=peak=true".to_owned());
    let mut command = Command::new(ffmpeg_path);
    command
        .args([
            "-hide_banner",
            "-nostats",
            "-nostdin",
            "-loglevel",
            "info",
            "-ss",
        ])
        .arg(seconds(request.start_ms))
        .arg("-t")
        .arg(seconds(request.duration_ms))
        .arg("-i")
        .arg(source_path)
        .args(["-map", "0:a:0", "-vn", "-sn", "-dn", "-af"])
        .arg(filters.join(","))
        .args(["-f", "null", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    configure_background_process(&mut command);
    let child = command.spawn().map_err(|error| {
        audio_energy_error(
            "audio_energy_ffmpeg_start_failed",
            "Ses enerjisi için FFmpeg başlatılamadı.",
            format!("Failed to start {}: {error}", ffmpeg_path.display()),
            true,
        )
    })?;
    let output = match analysis_cancel::wait_for_output(child, request.operation_id.as_deref()) {
        Ok(output) => output,
        Err(CommandWaitError::Cancelled) => {
            return Err(audio_energy_error(
                "smart_shorts_cancelled",
                "Akıllı Shorts analizi iptal edildi.",
                "Audio energy FFmpeg process cancelled",
                false,
            ));
        }
        Err(CommandWaitError::Io(error)) => {
            return Err(audio_energy_error(
                "audio_energy_ffmpeg_wait_failed",
                "Ses enerjisi analizi tamamlanamadı.",
                error.to_string(),
                true,
            ));
        }
    };
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        return Err(audio_energy_error_with_stderr(
            "audio_energy_ffmpeg_failed",
            "Klibin ses enerjisi çözümlenemedi.",
            format!("FFmpeg exited with {}", output.status),
            true,
            stderr_tail(&stderr),
        ));
    }
    let raw_points = parse_ebur128_points(&stderr, expected_duration_ms);
    if raw_points.is_empty() {
        return Err(audio_energy_error_with_stderr(
            "audio_energy_empty",
            "Klipte ölçülebilir bir ses yüksekliği eğrisi bulunamadı.",
            "FFmpeg ebur128 output contained no momentary loudness samples",
            false,
            stderr_tail(&stderr),
        ));
    }
    Ok(AnalyzeAudioEnergyResponse {
        engine: ENGINE_NAME,
        duration_ms: expected_duration_ms,
        points: bucket_points(&raw_points, BUCKET_MS),
    })
}

fn parse_ebur128_points(stderr: &str, duration_ms: u64) -> Vec<AudioEnergyPoint> {
    stderr
        .lines()
        .filter(|line| line.contains("ebur128") && line.contains(" M:"))
        .filter_map(|line| {
            let seconds = value_after_label(line, " t:")?;
            let loudness_db = value_after_label(line, " M:")?;
            if !seconds.is_finite() || !loudness_db.is_finite() || seconds < 0.0 {
                return None;
            }
            let at_ms = (seconds * 1_000.0).round().max(0.0) as u64;
            (at_ms <= duration_ms.saturating_add(250)).then_some(AudioEnergyPoint {
                at_ms: at_ms.min(duration_ms),
                loudness_db: (loudness_db * 10.0).round() / 10.0,
            })
        })
        .collect()
}

fn value_after_label(line: &str, label: &str) -> Option<f64> {
    line.split_once(label)?
        .1
        .trim_start()
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

fn bucket_points(points: &[AudioEnergyPoint], bucket_ms: u64) -> Vec<AudioEnergyPoint> {
    let bucket_ms = bucket_ms.max(1);
    let mut output = Vec::new();
    let mut current_bucket = u64::MAX;
    let mut current: Option<AudioEnergyPoint> = None;
    for point in points {
        let bucket = point.at_ms / bucket_ms;
        if bucket != current_bucket {
            if let Some(point) = current.take() {
                output.push(point);
            }
            current_bucket = bucket;
            current = Some(point.clone());
        } else if current
            .as_ref()
            .is_none_or(|existing| point.loudness_db > existing.loudness_db)
        {
            current = Some(point.clone());
        }
    }
    if let Some(point) = current {
        output.push(point);
    }
    output
}

fn validate_request(request: &AnalyzeAudioEnergyRequest) -> Result<(), RenderErrorReport> {
    if request.duration_ms == 0
        || request.duration_ms > MAX_DURATION_MS
        || !request.playback_rate.is_finite()
        || !(0.1..=16.0).contains(&request.playback_rate)
    {
        return Err(audio_energy_error(
            "audio_energy_request_invalid",
            "Ses enerjisi için klip aralığı veya hız geçersiz.",
            format!(
                "durationMs={}, playbackRate={}",
                request.duration_ms, request.playback_rate
            ),
            false,
        ));
    }
    Ok(())
}

fn validate_source_path(value: &str) -> Result<PathBuf, RenderErrorReport> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(audio_energy_error(
            "audio_energy_source_missing",
            "Ses enerjisi için kaynak medya seçilmedi.",
            "sourcePath was empty",
            false,
        ));
    }
    let path = fs::canonicalize(trimmed).map_err(|error| {
        audio_energy_error(
            "audio_energy_source_unavailable",
            "Kaynak medya ses enerjisi için açılamadı.",
            format!("{trimmed}: {error}"),
            false,
        )
    })?;
    if !path.is_file() {
        return Err(audio_energy_error(
            "audio_energy_source_invalid",
            "Ses enerjisi kaynağı bir dosya değil.",
            path.display().to_string(),
            false,
        ));
    }
    Ok(path)
}

fn atempo_filters(mut speed: f64) -> Vec<String> {
    let mut filters = Vec::new();
    while speed > 2.0 {
        filters.push("atempo=2".to_owned());
        speed /= 2.0;
    }
    while speed < 0.5 {
        filters.push("atempo=0.5".to_owned());
        speed /= 0.5;
    }
    if (speed - 1.0).abs() > 0.000_001 {
        filters.push(format!("atempo={speed:.8}"));
    }
    filters
}

fn post_speed_duration_ms(duration_ms: u64, playback_rate: f64) -> u64 {
    ((duration_ms as f64 / playback_rate).round() as u64).max(1)
}

fn seconds(duration_ms: u64) -> String {
    format!("{:.6}", duration_ms as f64 / 1_000.0)
}

fn stderr_tail(value: &str) -> Vec<String> {
    let lines = value.lines().collect::<Vec<_>>();
    lines[lines.len().saturating_sub(24)..]
        .iter()
        .map(|line| (*line).to_owned())
        .collect()
}

fn audio_energy_error(
    code: impl Into<String>,
    user_message: impl Into<String>,
    technical_message: impl Into<String>,
    retryable: bool,
) -> RenderErrorReport {
    audio_energy_error_with_stderr(code, user_message, technical_message, retryable, Vec::new())
}

fn audio_energy_error_with_stderr(
    code: impl Into<String>,
    user_message: impl Into<String>,
    technical_message: impl Into<String>,
    retryable: bool,
    stderr_tail: Vec<String>,
) -> RenderErrorReport {
    RenderErrorReport {
        code: code.into(),
        user_message: user_message.into(),
        technical_message: technical_message.into(),
        retryable,
        stderr_tail,
    }
}

fn default_playback_rate() -> f64 {
    1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_reads_momentary_loudness_and_ignores_summary_lines() {
        let stderr = r#"
[Parsed_ebur128_0 @ 000001] t: 0.0999792 TARGET:-23 LUFS    M: -120.7 S: -120.7 I: -70.0 LUFS
[Parsed_ebur128_0 @ 000001] t: 0.199979 TARGET:-23 LUFS    M: -31.25 S: -120.7 I: -31.2 LUFS
[Parsed_ebur128_0 @ 000001] Summary:
"#;
        assert_eq!(
            parse_ebur128_points(stderr, 1_000),
            vec![
                AudioEnergyPoint {
                    at_ms: 100,
                    loudness_db: -120.7,
                },
                AudioEnergyPoint {
                    at_ms: 200,
                    loudness_db: -31.3,
                },
            ]
        );
    }

    #[test]
    fn bucket_keeps_the_strongest_sample_per_quarter_second() {
        let points = vec![
            AudioEnergyPoint {
                at_ms: 100,
                loudness_db: -31.0,
            },
            AudioEnergyPoint {
                at_ms: 200,
                loudness_db: -18.0,
            },
            AudioEnergyPoint {
                at_ms: 300,
                loudness_db: -24.0,
            },
        ];
        assert_eq!(
            bucket_points(&points, 250),
            vec![points[1].clone(), points[2].clone()]
        );
    }

    #[test]
    fn speed_mapping_uses_post_speed_timeline_duration() {
        assert_eq!(post_speed_duration_ms(30_000, 2.0), 15_000);
        assert_eq!(post_speed_duration_ms(30_000, 0.5), 60_000);
    }

    #[test]
    #[ignore = "requires ASTRAL_FFMPEG_SMOKE to point to a real FFmpeg executable"]
    fn real_ffmpeg_emits_a_timestamped_energy_curve() {
        let ffmpeg = PathBuf::from(
            std::env::var("ASTRAL_FFMPEG_SMOKE")
                .expect("ASTRAL_FFMPEG_SMOKE must point to ffmpeg.exe"),
        );
        let source = std::env::temp_dir().join(format!(
            "astral-audio-energy-smoke-{}-{}.wav",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let generated = Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
            ])
            .arg("sine=frequency=440:duration=2")
            .arg(&source)
            .status()
            .expect("generate sine wave");
        assert!(generated.success());
        let request = AnalyzeAudioEnergyRequest {
            source_path: source.display().to_string(),
            start_ms: 0,
            duration_ms: 2_000,
            playback_rate: 1.0,
            ffmpeg_path: None,
            operation_id: None,
        };
        let result = analyze_audio_energy_blocking(&ffmpeg, &source, &request).unwrap();
        let _ = fs::remove_file(&source);
        assert_eq!(result.duration_ms, 2_000);
        assert!(result.points.len() >= 5);
        assert!(result
            .points
            .windows(2)
            .all(|pair| pair[0].at_ms <= pair[1].at_ms));
    }
}
