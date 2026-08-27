#!/usr/bin/env python3
"""Local OpenCV ViTTrack runner used by Astral Lunar's auto-reframe backend.

The Rust host owns path/range validation and the cache. This runner accepts only
explicit arguments (never a shell command), writes one compact JSON result, and
uses stderr for small machine-readable progress messages. OpenCV ViTTrack is
preferred; the classic CPU-only CSRT tracker remains a recoverable fallback.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import sys
from pathlib import Path
from typing import Any


SCHEMA = "astral-auto-reframe-track-v1"
ENGINE = "opencv-vittrack-csrt-fallback"
CONFIDENCE_METRIC = "vittrack-score-or-binary-fallback"
PROGRESS_PREFIX = "ASTRAL_PROGRESS "
MIN_TRACKER_UPDATE_FPS = 12.0
MIN_VIT_TRACKING_SCORE = 0.18
BACKWARD_CHUNK_MS = 2_000.0
TRACKING_PROGRESS_START = 2.0
TRACKING_PROGRESS_END = 98.0


def emit_progress(stage: str, percent: float, message: str) -> None:
    payload = {
        "stage": stage,
        "progressPercent": max(0.0, min(100.0, float(percent))),
        "message": message,
    }
    print(PROGRESS_PREFIX + json.dumps(payload, separators=(",", ":")), file=sys.stderr, flush=True)


class ProgressReporter:
    """Emits throttled, monotonic progress across both tracking directions."""

    def __init__(self, estimated_units: int) -> None:
        self.estimated_units = max(1, estimated_units)
        self.processed_units = 0
        self.last_percent = 0.0
        self.last_tick = -1
        self.last_stage = ""
        self.last_message = ""

    def phase(self, stage: str, message: str) -> None:
        self._emit(
            stage,
            self._percent(),
            message,
            force=stage != self.last_stage or message != self.last_message,
        )

    def advance(self, stage: str, message: str) -> None:
        self.processed_units += 1
        percent = self._percent()
        self._emit(stage, percent, message, force=int(percent) > self.last_tick)

    def complete(self) -> None:
        self._emit("complete", 100.0, "Yerel nesne takibi tamamlandı.", force=True)

    def _percent(self) -> float:
        fraction = min(1.0, self.processed_units / self.estimated_units)
        return TRACKING_PROGRESS_START + (
            TRACKING_PROGRESS_END - TRACKING_PROGRESS_START
        ) * fraction

    def _emit(self, stage: str, percent: float, message: str, force: bool) -> None:
        percent = max(self.last_percent, min(100.0, percent))
        tick = int(percent)
        if not force and tick <= self.last_tick:
            return
        emit_progress(stage, percent, message)
        self.last_percent = percent
        self.last_tick = tick
        self.last_stage = stage
        self.last_message = message


def create_csrt_tracker(cv2: Any) -> Any:
    factory = getattr(cv2, "TrackerCSRT_create", None)
    if factory is None and hasattr(cv2, "legacy"):
        factory = getattr(cv2.legacy, "TrackerCSRT_create", None)
    if factory is None:
        raise RuntimeError(
            "OpenCV CSRT tracker is unavailable; opencv-contrib-python-headless is required"
        )
    return factory()


def create_vit_tracker(cv2: Any, model: Path) -> Any:
    if not model.is_file():
        raise RuntimeError("ViTTrack ONNX model file is unavailable")

    params_factory = getattr(cv2, "TrackerVit_Params", None)
    if params_factory is None:
        tracker_class = getattr(cv2, "TrackerVit", None)
        params_factory = getattr(tracker_class, "Params", None)
    factory = getattr(cv2, "TrackerVit_create", None)
    if factory is None:
        tracker_class = getattr(cv2, "TrackerVit", None)
        factory = getattr(tracker_class, "create", None)
    if not callable(params_factory) or not callable(factory):
        raise RuntimeError("OpenCV TrackerVit API is unavailable")

    parameters = params_factory()
    parameters.net = os.fspath(model)
    # The bundled PyPI OpenCV wheel uses its CPU DNN backend. Keep this explicit
    # so the result never implies CUDA acceleration that the runtime does not have.
    dnn = getattr(cv2, "dnn", None)
    if dnn is not None:
        backend = getattr(dnn, "DNN_BACKEND_OPENCV", None)
        target = getattr(dnn, "DNN_TARGET_CPU", None)
        if backend is not None:
            parameters.backend = backend
        if target is not None:
            parameters.target = target
    tracker = factory(parameters)
    if tracker is None or not callable(getattr(tracker, "getTrackingScore", None)):
        raise RuntimeError("OpenCV TrackerVit tracking score API is unavailable")
    return tracker


def recover_with_csrt(
    cv2: Any,
    frame: Any,
    last_confident_box: list[float],
) -> tuple[Any, list[float]]:
    height, width = frame.shape[:2]
    initial = pixel_box(last_confident_box, width, height)
    tracker = create_csrt_tracker(cv2)
    initialized = tracker.init(frame, initial)
    if initialized is False:
        raise RuntimeError("CSRT rejected the last confident ViTTrack box")
    return tracker, normalized_box(initial, width, height)


def resize_for_analysis(cv2: Any, frame: Any, max_dimension: int) -> Any:
    height, width = frame.shape[:2]
    largest = max(width, height)
    if largest <= max_dimension:
        return frame
    scale = max_dimension / float(largest)
    resized_width = max(2, int(round(width * scale)))
    resized_height = max(2, int(round(height * scale)))
    return cv2.resize(frame, (resized_width, resized_height), interpolation=cv2.INTER_AREA)


def pixel_box(normalized_box: list[float], width: int, height: int) -> tuple[int, int, int, int]:
    x, y, box_width, box_height = normalized_box
    left = max(0, min(width - 2, int(math.floor(x * width))))
    top = max(0, min(height - 2, int(math.floor(y * height))))
    right = max(left + 2, min(width, int(math.ceil((x + box_width) * width))))
    bottom = max(top + 2, min(height, int(math.ceil((y + box_height) * height))))
    return left, top, right - left, bottom - top


def normalized_box(box: tuple[float, float, float, float], width: int, height: int) -> list[float]:
    x, y, box_width, box_height = [float(value) for value in box]
    left = max(0.0, min(float(width), x))
    top = max(0.0, min(float(height), y))
    right = max(left, min(float(width), x + box_width))
    bottom = max(top, min(float(height), y + box_height))
    return [
        left / width,
        top / height,
        (right - left) / width,
        (bottom - top) / height,
    ]


def read_seed_frame(cv2: Any, capture: Any, absolute_time_ms: float, max_dimension: int) -> Any | None:
    # One seek per direction is acceptable. Tracking updates after this read are
    # driven by sequential decode, never random seek-per-sample.
    capture.set(cv2.CAP_PROP_POS_MSEC, max(0.0, absolute_time_ms))
    ok, frame = capture.read()
    if not ok or frame is None:
        return None
    return resize_for_analysis(cv2, frame, max_dimension)


def init_tracker(
    cv2: Any,
    seed_frame: Any,
    selection: list[float],
    model: Path,
) -> tuple[Any, list[float], str, list[str]]:
    height, width = seed_frame.shape[:2]
    initial = pixel_box(selection, width, height)
    warnings: list[str] = []

    try:
        tracker = create_vit_tracker(cv2, model)
        initialized = tracker.init(seed_frame, initial)
        if initialized is False:
            raise RuntimeError("ViTTrack rejected the selected object box")
        return tracker, normalized_box(initial, width, height), "vittrack-cpu", warnings
    except Exception as error:
        warnings.append(
            "OpenCV ViTTrack başlatılamadı "
            f"({type(error).__name__}); CPU CSRT geri dönüşü etkinleştirildi."
        )

    tracker = create_csrt_tracker(cv2)
    initialized = tracker.init(seed_frame, initial)
    if initialized is False:
        raise RuntimeError("CSRT rejected the selected object box at the seed frame")
    return tracker, normalized_box(initial, width, height), "csrt-cpu-fallback", warnings


def update_tracker(
    tracker: Any,
    tracker_mode: str,
    frame: Any,
) -> tuple[bool, bool, Any, float]:
    updated, tracked = tracker.update(frame)
    if not updated:
        return False, False, tracked, 0.0
    if tracker_mode != "vittrack-cpu":
        return True, True, tracked, 1.0

    try:
        score = float(tracker.getTrackingScore())
    except Exception:
        return True, False, tracked, 0.0
    if not math.isfinite(score):
        return True, False, tracked, 0.0
    confidence = max(0.0, min(1.0, score))
    return True, confidence >= MIN_VIT_TRACKING_SCORE, tracked, confidence


def safe_source_fps(source_fps: float) -> float:
    return source_fps if math.isfinite(source_fps) and 0.1 <= source_fps <= 1_000.0 else 30.0


def tracker_stride(source_fps: float) -> int:
    # Decode every frame but run the tracker update at >=~12 fps when the source
    # frame rate permits it. Low-fps sources are updated every frame.
    return max(1, int(source_fps / MIN_TRACKER_UPDATE_FPS))


def estimated_update_units(duration_ms: int, source_fps: float, stride: int) -> int:
    update_fps = source_fps / max(1, stride)
    return max(1, int(math.ceil(max(0, duration_ms) / 1_000.0 * update_fps)))


def append_sample(
    samples: list[dict[str, Any]],
    relative_ms: float,
    duration_ms: int,
    bbox: list[float],
    confidence: float,
) -> None:
    # The analyzed clip range is half-open: [0, duration_ms). Never leak the
    # first frame after the clip into smoothing or export keyframes.
    timestamp = max(0, min(duration_ms - 1, int(round(relative_ms))))
    samples.append({"t": timestamp, "b": bbox, "c": confidence})


def track_forward(
    cv2: Any,
    model: Path,
    source: str,
    start_ms: int,
    duration_ms: int,
    seed_time_ms: int,
    selection: list[float],
    sample_fps: float,
    max_dimension: int,
    source_fps: float,
    stride: int,
    progress: ProgressReporter,
) -> tuple[list[dict[str, Any]], list[str], str]:
    capture = cv2.VideoCapture(source)
    if not capture.isOpened():
        raise RuntimeError("OpenCV could not open the selected video")

    seed_frame = read_seed_frame(cv2, capture, start_ms + seed_time_ms, max_dimension)
    if seed_frame is None:
        capture.release()
        raise RuntimeError("OpenCV could not decode the selected seed frame")
    tracker, last_box, tracker_mode, warnings = init_tracker(
        cv2, seed_frame, selection, model
    )
    last_confident_box = last_box

    stage = "tracking-forward"
    label = (
        "Nesne ViTTrack ile ileri yönde takip ediliyor."
        if tracker_mode == "vittrack-cpu"
        else "Nesne CSRT geri dönüşüyle ileri yönde takip ediliyor."
    )
    progress.phase(stage, label)
    samples: list[dict[str, Any]] = []
    consecutive_failures = 0
    warning: str | None = None
    frame_duration_ms = 1_000.0 / source_fps
    output_interval_ms = 1_000.0 / sample_fps
    next_output_ms = seed_time_ms + output_interval_ms
    decoded_index = 0
    try:
        while True:
            decoded_index += 1
            relative_ms = seed_time_ms + decoded_index * frame_duration_ms
            if relative_ms >= duration_ms:
                break
            ok, frame = capture.read()
            if not ok or frame is None:
                warning = (
                    "Video ileri yönde istenen zaman aralığının sonuna kadar çözülemedi; "
                    "takip mevcut örneklerde durduruldu."
                )
                break
            if decoded_index % stride != 0:
                continue

            frame = resize_for_analysis(cv2, frame, max_dimension)
            update_returned, confident, tracked, confidence = update_tracker(
                tracker, tracker_mode, frame
            )
            if update_returned:
                height, width = frame.shape[:2]
                last_box = normalized_box(tracked, width, height)
            if confident:
                consecutive_failures = 0
                last_confident_box = last_box
            else:
                consecutive_failures += 1
            progress.advance(stage, label)

            if relative_ms + 1e-6 >= next_output_ms:
                append_sample(samples, relative_ms, duration_ms, last_box, confidence)
                while next_output_ms <= relative_ms + 1e-6:
                    next_output_ms += output_interval_ms

            if consecutive_failures >= 3:
                if tracker_mode == "vittrack-cpu":
                    try:
                        tracker, last_box = recover_with_csrt(
                            cv2, frame, last_confident_box
                        )
                        tracker_mode = "vit-csrt-cpu"
                        consecutive_failures = 0
                        label = "ViTTrack güveni düştü; CPU CSRT takibi devraldı."
                        progress.phase(stage, label)
                        warnings.append(
                            "ViTTrack ileri yönde 0.18 güven eşiğinin altında kaldı; "
                            "son güvenilir kutudan CPU CSRT geri dönüşü otomatik devraldı."
                        )
                        continue
                    except Exception as error:
                        warning = (
                            "ViTTrack ileri yönde güvenini kaybetti ve CSRT geri dönüşü "
                            f"başlatılamadı ({type(error).__name__}). Son güvenilir kutu korundu; "
                            "bu bölümde kullanıcı düzeltmesi gerekir."
                        )
                else:
                    warning = (
                        "CSRT seçilen nesneyi ileri yönde kaybetti. Son güvenilir kutu korundu; "
                        "bu bölümde kullanıcı düzeltmesi gerekir."
                    )
                last_box = last_confident_box
                append_sample(samples, relative_ms, duration_ms, last_box, 0.0)
                break
    finally:
        capture.release()
    if warning:
        warnings.append(warning)
    return samples, warnings, tracker_mode


def decode_backward_chunk(
    cv2: Any,
    source: str,
    start_ms: int,
    chunk_start_ms: float,
    chunk_end_ms: float,
    max_dimension: int,
    source_fps: float,
    stride: int,
) -> list[tuple[float, Any]]:
    """Sequentially decodes one short chunk and retains only tracker-rate frames."""
    capture = cv2.VideoCapture(source)
    if not capture.isOpened():
        raise RuntimeError("OpenCV could not open the selected video")
    capture.set(cv2.CAP_PROP_POS_MSEC, max(0.0, start_ms + chunk_start_ms))
    frame_duration_ms = 1_000.0 / source_fps
    frames: list[tuple[float, Any]] = []
    decoded_index = 0
    try:
        while True:
            relative_ms = chunk_start_ms + decoded_index * frame_duration_ms
            if relative_ms >= chunk_end_ms:
                break
            ok, frame = capture.read()
            if not ok or frame is None:
                break
            if decoded_index % stride == 0:
                frames.append((relative_ms, resize_for_analysis(cv2, frame, max_dimension)))
            decoded_index += 1
    finally:
        capture.release()
    return frames


def track_backward(
    cv2: Any,
    model: Path,
    source: str,
    start_ms: int,
    duration_ms: int,
    seed_time_ms: int,
    selection: list[float],
    sample_fps: float,
    max_dimension: int,
    source_fps: float,
    stride: int,
    progress: ProgressReporter,
) -> tuple[list[dict[str, Any]], list[str], str]:
    seed_capture = cv2.VideoCapture(source)
    if not seed_capture.isOpened():
        raise RuntimeError("OpenCV could not open the selected video")
    try:
        seed_frame = read_seed_frame(
            cv2, seed_capture, start_ms + seed_time_ms, max_dimension
        )
    finally:
        seed_capture.release()
    if seed_frame is None:
        raise RuntimeError("OpenCV could not decode the selected seed frame")
    tracker, last_box, tracker_mode, warnings = init_tracker(
        cv2, seed_frame, selection, model
    )
    last_confident_box = last_box

    stage = "tracking-backward"
    label = (
        "Nesne ViTTrack ile sınırlı bellekli olarak geriye takip ediliyor."
        if tracker_mode == "vittrack-cpu"
        else "Nesne CSRT geri dönüşüyle sınırlı bellekli olarak geriye takip ediliyor."
    )
    progress.phase(stage, label)
    output_interval_ms = 1_000.0 / sample_fps
    next_output_ms = seed_time_ms - output_interval_ms
    cursor_end_ms = float(seed_time_ms)
    samples: list[dict[str, Any]] = []
    warning: str | None = None
    consecutive_failures = 0

    while cursor_end_ms > 0.0:
        chunk_start_ms = max(0.0, cursor_end_ms - BACKWARD_CHUNK_MS)
        frames = decode_backward_chunk(
            cv2,
            source,
            start_ms,
            chunk_start_ms,
            cursor_end_ms,
            max_dimension,
            source_fps,
            stride,
        )
        if not frames:
            warning = (
                "Video geriye doğru istenen zaman aralığının sonuna kadar çözülemedi; "
                "takip mevcut örneklerde durduruldu."
            )
            break

        for relative_ms, frame in reversed(frames):
            update_returned, confident, tracked, confidence = update_tracker(
                tracker, tracker_mode, frame
            )
            if update_returned:
                height, width = frame.shape[:2]
                last_box = normalized_box(tracked, width, height)
            if confident:
                consecutive_failures = 0
                last_confident_box = last_box
            else:
                consecutive_failures += 1
            progress.advance(stage, label)

            if relative_ms <= next_output_ms + 1e-6:
                append_sample(samples, relative_ms, duration_ms, last_box, confidence)
                while next_output_ms >= relative_ms - 1e-6:
                    next_output_ms -= output_interval_ms

            if consecutive_failures >= 3:
                if tracker_mode == "vittrack-cpu":
                    try:
                        tracker, last_box = recover_with_csrt(
                            cv2, frame, last_confident_box
                        )
                        tracker_mode = "vit-csrt-cpu"
                        consecutive_failures = 0
                        label = "ViTTrack güveni düştü; CPU CSRT geriye takibi devraldı."
                        progress.phase(stage, label)
                        warnings.append(
                            "ViTTrack geriye doğru 0.18 güven eşiğinin altında kaldı; "
                            "son güvenilir kutudan CPU CSRT geri dönüşü otomatik devraldı."
                        )
                        continue
                    except Exception as error:
                        warning = (
                            "ViTTrack geriye doğru güvenini kaybetti ve CSRT geri dönüşü "
                            f"başlatılamadı ({type(error).__name__}). Son güvenilir kutu korundu; "
                            "bu bölümde kullanıcı düzeltmesi gerekir."
                        )
                else:
                    warning = (
                        "CSRT seçilen nesneyi geriye doğru kaybetti. Son güvenilir kutu korundu; "
                        "bu bölümde kullanıcı düzeltmesi gerekir."
                    )
                last_box = last_confident_box
                append_sample(samples, relative_ms, duration_ms, last_box, 0.0)
                warnings.append(warning)
                return samples, warnings, tracker_mode

        cursor_end_ms = chunk_start_ms

    if warning:
        warnings.append(warning)
    return samples, warnings, tracker_mode


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Astral Lunar local ViTTrack tracker with CSRT fallback"
    )
    parser.add_argument("--model", required=True)
    parser.add_argument("--source", required=True)
    parser.add_argument("--start-ms", required=True, type=int)
    parser.add_argument("--duration-ms", required=True, type=int)
    parser.add_argument("--seed-time-ms", required=True, type=int)
    parser.add_argument("--box", required=True, nargs=4, type=float)
    parser.add_argument("--sample-fps", required=True, type=float)
    parser.add_argument("--max-dimension", required=True, type=int)
    parser.add_argument("--request-key", required=True)
    parser.add_argument("--output", required=True)
    return parser.parse_args()


def run() -> None:
    args = parse_args()
    import cv2  # Imported after argument parsing so setup diagnostics stay clear.

    emit_progress("opening", 0.0, "Video yerel takip motorunda açılıyor.")
    probe = cv2.VideoCapture(args.source)
    if not probe.isOpened():
        raise RuntimeError("OpenCV could not open the selected video")
    source_width = int(round(probe.get(cv2.CAP_PROP_FRAME_WIDTH)))
    source_height = int(round(probe.get(cv2.CAP_PROP_FRAME_HEIGHT)))
    source_fps = float(probe.get(cv2.CAP_PROP_FPS))
    probe.release()
    if source_width <= 0 or source_height <= 0:
        raise RuntimeError("OpenCV reported invalid source dimensions")

    tracking_fps = safe_source_fps(source_fps)
    stride = tracker_stride(tracking_fps)
    estimated_units = estimated_update_units(args.duration_ms, tracking_fps, stride)
    progress = ProgressReporter(estimated_units)
    selection = [float(value) for value in args.box]
    model = Path(args.model)
    backward, backward_warnings, backward_mode = track_backward(
        cv2,
        model,
        args.source,
        args.start_ms,
        args.duration_ms,
        args.seed_time_ms,
        selection,
        args.sample_fps,
        args.max_dimension,
        tracking_fps,
        stride,
        progress,
    )
    forward, forward_warnings, forward_mode = track_forward(
        cv2,
        model,
        args.source,
        args.start_ms,
        args.duration_ms,
        args.seed_time_ms,
        selection,
        args.sample_fps,
        args.max_dimension,
        tracking_fps,
        stride,
        progress,
    )

    samples = backward + [{"t": args.seed_time_ms, "b": selection, "c": 1.0}] + forward
    samples.sort(key=lambda sample: sample["t"])
    # Rounding can only collide at a direction boundary; retain the stronger sample.
    deduplicated: dict[int, dict[str, Any]] = {}
    for sample in samples:
        previous = deduplicated.get(sample["t"])
        if previous is None or sample["c"] > previous["c"]:
            deduplicated[sample["t"]] = sample

    warnings = list(dict.fromkeys(backward_warnings + forward_warnings))
    if backward_mode == forward_mode:
        engine_version = f"{cv2.__version__};mode={backward_mode}"
    else:
        engine_version = (
            f"{cv2.__version__};back={backward_mode};forward={forward_mode}"
        )
    result = {
        "schema": SCHEMA,
        "requestKey": args.request_key,
        "engine": ENGINE,
        "engineVersion": engine_version,
        "sourceWidth": source_width,
        "sourceHeight": source_height,
        "sourceFps": source_fps if math.isfinite(source_fps) and source_fps > 0 else None,
        "confidenceMetric": CONFIDENCE_METRIC,
        "samples": list(deduplicated.values()),
        "warnings": warnings,
    }

    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(result, handle, ensure_ascii=False, separators=(",", ":"), allow_nan=False)
        handle.write("\n")
        handle.flush()
        os.fsync(handle.fileno())
    progress.complete()


if __name__ == "__main__":
    try:
        run()
    except Exception as error:  # The Rust host turns this into a structured diagnostic.
        print(f"AUTO_REFRAME_ERROR {type(error).__name__}: {error}", file=sys.stderr, flush=True)
        raise SystemExit(1)
