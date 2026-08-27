#!/usr/bin/env python3
"""Run the official YAMNet AudioSet classifier for laughter-family events.

Astral Lunar extracts clip-local, post-speed mono float32 audio with FFmpeg.
This runner keeps the ML boundary deliberately small: MediaPipe supplies the
official YAMNet labels and scores; Rust validates the output and turns adjacent
positive windows into editor events.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import sys
from pathlib import Path

import numpy as np
from mediapipe.tasks import python
from mediapipe.tasks.python import audio
from mediapipe.tasks.python.components.containers import AudioData


SCHEMA = "astral-yamnet-laughter-v1"
SAMPLE_RATE = 16_000
WINDOW_MS = 960
HOP_MS = 480
CHUNK_MS = 5 * 60 * 1_000
LAUGHTER_LABELS = (
    "Laughter",
    "Baby laughter",
    "Giggle",
    "Snicker",
    "Belly laugh",
    "Chuckle, chortle",
)
PROGRESS_PREFIX = "ASTRAL_PROGRESS "


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True)
    parser.add_argument("--audio", required=True)
    parser.add_argument("--duration-ms", required=True, type=int)
    parser.add_argument("--output", required=True)
    return parser.parse_args()


def fail(message: str) -> "None":
    raise SystemExit(message)


def emit_progress(progress_percent: float, message: str) -> None:
    payload = {
        "progressPercent": round(max(0.0, min(100.0, progress_percent)), 2),
        "message": message,
    }
    print(PROGRESS_PREFIX + json.dumps(payload, separators=(",", ":")), file=sys.stderr, flush=True)


def classify(model_path: Path, audio_path: Path, duration_ms: int) -> dict[str, object]:
    if not model_path.is_file():
        fail(f"YAMNet model does not exist: {model_path}")
    if not audio_path.is_file():
        fail(f"PCM input does not exist: {audio_path}")
    if duration_ms <= 0:
        fail("duration-ms must be positive")

    byte_count = audio_path.stat().st_size
    if byte_count <= 0 or byte_count % 4 != 0:
        fail(f"PCM input size is invalid: {byte_count}")
    samples = np.memmap(audio_path, dtype="<f4", mode="r")
    if samples.size == 0:
        fail("PCM input is empty")

    options = audio.AudioClassifierOptions(
        base_options=python.BaseOptions(model_asset_path=os.fspath(model_path)),
        max_results=-1,
        score_threshold=0.0,
        category_allowlist=list(LAUGHTER_LABELS),
    )

    chunk_samples = CHUNK_MS * SAMPLE_RATE // 1_000
    overlap_samples = WINDOW_MS * SAMPLE_RATE // 1_000
    step_samples = chunk_samples - overlap_samples
    scored_frames: list[tuple[int, float, str]] = []
    seen_labels: set[str] = set()

    with audio.AudioClassifier.create_from_options(options) as classifier:
        # The MediaPipe task wrapper advances this pinned model by about 975 ms.
        # A second pass shifted by 480 ms restores the original YAMNet-style
        # short-event coverage without inventing events from signal energy.
        pass_offsets = (0, HOP_MS)
        pass_starts: list[list[int]] = []
        for pass_offset_ms in pass_offsets:
            starts: list[int] = []
            chunk_start = pass_offset_ms * SAMPLE_RATE // 1_000
            while chunk_start < samples.size:
                starts.append(chunk_start)
                if chunk_start + chunk_samples >= samples.size:
                    break
                chunk_start += step_samples
            pass_starts.append(starts)
        total_chunks = sum(len(starts) for starts in pass_starts)
        completed_chunks = 0

        for starts in pass_starts:
            for chunk_start in starts:
                chunk_end = min(samples.size, chunk_start + chunk_samples)
                # MediaPipe owns a contiguous float32 copy for classify().
                chunk = np.asarray(samples[chunk_start:chunk_end], dtype=np.float32).copy()
                if not np.isfinite(chunk).all():
                    fail("PCM input contains non-finite samples")
                np.clip(chunk, -1.0, 1.0, out=chunk)
                results = classifier.classify(
                    AudioData.create_from_array(chunk, sample_rate=SAMPLE_RATE)
                )
                chunk_offset_ms = round(chunk_start * 1_000 / SAMPLE_RATE)
                for result in results:
                    timestamp_ms = chunk_offset_ms + int(result.timestamp_ms)
                    if timestamp_ms >= duration_ms:
                        continue
                    categories = result.classifications[0].categories
                    if not categories:
                        fail("YAMNet returned an empty laughter category list")
                    for category in categories:
                        if category.category_name:
                            seen_labels.add(category.category_name)
                    best = max(categories, key=lambda category: float(category.score))
                    score = float(best.score)
                    if not math.isfinite(score):
                        fail("YAMNet returned a non-finite score")
                    scored_frames.append(
                        (
                            timestamp_ms,
                            max(0.0, min(1.0, score)),
                            best.category_name,
                        )
                    )
                completed_chunks += 1
                emit_progress(
                    completed_chunks / max(1, total_chunks) * 100.0,
                    "YAMNet laughter windows classified",
                )

    missing_labels = set(LAUGHTER_LABELS).difference(seen_labels)
    if missing_labels:
        fail("YAMNet laughter label contract changed: " + ", ".join(sorted(missing_labels)))
    if not scored_frames:
        fail("YAMNet returned no timestamped frames")

    # Chunk overlap may produce an identical timestamp twice. Keep the stronger
    # neural score, then serialize strictly increasing clip-local timestamps.
    by_timestamp: dict[int, tuple[float, str]] = {}
    for timestamp_ms, score, label in scored_frames:
        previous = by_timestamp.get(timestamp_ms)
        if previous is None or score > previous[0]:
            by_timestamp[timestamp_ms] = (score, label)
    frames = [
        {"t": timestamp_ms, "s": round(score, 7), "l": label}
        for timestamp_ms, (score, label) in sorted(by_timestamp.items())
    ]

    return {
        "schema": SCHEMA,
        "sampleRate": SAMPLE_RATE,
        "windowMs": WINDOW_MS,
        "hopMs": HOP_MS,
        "durationMs": duration_ms,
        "frames": frames,
    }


def main() -> int:
    args = parse_args()
    payload = classify(
        Path(args.model).resolve(),
        Path(args.audio).resolve(),
        args.duration_ms,
    )
    output_path = Path(args.output).resolve()
    with output_path.open("x", encoding="utf-8", newline="\n") as output_file:
        json.dump(payload, output_file, ensure_ascii=True, separators=(",", ":"))
        output_file.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
