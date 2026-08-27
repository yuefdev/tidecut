#!/usr/bin/env python3
"""Local OpenCV facecam detector used by Astral Lunar's streamer layout.

Finds the streamer webcam region automatically: sparse frames are sampled
across the clip, Haar-cascade face detection runs on each, and detections are
clustered over time. The most stable cluster is the facecam; when the overlay
moves to another corner mid-video a new cluster takes over. Output uses the
same compact sample schema as the tracker so the camera planner can consume it
directly. The Rust host owns path validation and the cache.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import sys
from pathlib import Path
from typing import Any

SCHEMA = "astral-facecam-detect-v1"
ENGINE = "opencv-haar-facecam"
CONFIDENCE_METRIC = "haar-cluster-stability"
PROGRESS_PREFIX = "ASTRAL_PROGRESS "

# A cluster must collect this many hits inside the stability window before it
# can drive the layout; corner facecams hit nearly every sample while incidental
# gameplay faces spread across many short-lived clusters.
STABILITY_WINDOW_MS = 40_000.0
MIN_ACTIVATION_HITS = 3
SWITCH_HYSTERESIS_HITS = 2
MAX_CLUSTERS = 24
EMA_ALPHA = 0.25
HELD_CONFIDENCE = 0.35
BACKFILL_CONFIDENCE = 0.4


def emit_progress(stage: str, percent: float, message: str) -> None:
    payload = {
        "stage": stage,
        "progressPercent": max(0.0, min(100.0, float(percent))),
        "message": message,
    }
    print(PROGRESS_PREFIX + json.dumps(payload, separators=(",", ":")), file=sys.stderr, flush=True)


def clamp(value: float, minimum: float, maximum: float) -> float:
    return min(maximum, max(minimum, value))


class FaceCluster:
    __slots__ = ("box", "hit_times", "total_hits", "created_ms")

    def __init__(self, box: list[float], time_ms: float) -> None:
        self.box = list(box)
        self.hit_times: list[float] = [time_ms]
        self.total_hits = 1
        self.created_ms = time_ms

    def matches(self, box: list[float]) -> bool:
        cx = self.box[0] + self.box[2] / 2
        cy = self.box[1] + self.box[3] / 2
        ox = box[0] + box[2] / 2
        oy = box[1] + box[3] / 2
        mean_width = max(1e-6, (self.box[2] + box[2]) / 2)
        distance = math.hypot(cx - ox, cy - oy)
        size_ratio = box[2] / max(1e-6, self.box[2])
        return distance <= mean_width * 0.9 and 0.45 <= size_ratio <= 2.2

    def absorb(self, box: list[float], time_ms: float) -> None:
        for index in range(4):
            self.box[index] += (box[index] - self.box[index]) * EMA_ALPHA
        self.hit_times.append(time_ms)
        self.total_hits += 1

    def recent_hits(self, now_ms: float) -> int:
        cutoff = now_ms - STABILITY_WINDOW_MS
        while self.hit_times and self.hit_times[0] < cutoff:
            self.hit_times.pop(0)
        return len(self.hit_times)


def expand_face_to_panel(box: list[float]) -> list[float]:
    """Grows a raw face box into a head-and-shoulders facecam panel box."""
    x, y, width, height = box
    center_x = x + width / 2
    left = clamp(center_x - width * 1.05, 0.0, 1.0)
    right = clamp(center_x + width * 1.05, 0.0, 1.0)
    top = clamp(y - height * 0.55, 0.0, 1.0)
    bottom = clamp(y + height * 1.75, 0.0, 1.0)
    return [left, top, max(1e-6, right - left), max(1e-6, bottom - top)]


def detect_faces(cv2: Any, cascade: Any, frame: Any) -> list[list[float]]:
    gray = cv2.cvtColor(frame, cv2.COLOR_BGR2GRAY)
    gray = cv2.equalizeHist(gray)
    height, width = gray.shape[:2]
    smallest = min(width, height)
    minimum = max(20, int(smallest * 0.04))
    detections = cascade.detectMultiScale(
        gray,
        scaleFactor=1.1,
        minNeighbors=6,
        minSize=(minimum, minimum),
        maxSize=(int(smallest * 0.6), int(smallest * 0.6)),
    )
    boxes: list[list[float]] = []
    for x, y, box_width, box_height in detections:
        boxes.append(
            [
                float(x) / width,
                float(y) / height,
                float(box_width) / width,
                float(box_height) / height,
            ]
        )
    return boxes


def resize_for_analysis(cv2: Any, frame: Any, max_dimension: int) -> Any:
    height, width = frame.shape[:2]
    largest = max(width, height)
    if largest <= max_dimension:
        return frame
    scale = max_dimension / float(largest)
    resized_width = max(2, int(round(width * scale)))
    resized_height = max(2, int(round(height * scale)))
    return cv2.resize(frame, (resized_width, resized_height), interpolation=cv2.INTER_AREA)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Astral Lunar facecam region detector")
    parser.add_argument("--source", required=True)
    parser.add_argument("--start-ms", required=True, type=int)
    parser.add_argument("--duration-ms", required=True, type=int)
    parser.add_argument("--interval-ms", required=True, type=int)
    parser.add_argument("--max-dimension", required=True, type=int)
    parser.add_argument("--request-key", required=True)
    parser.add_argument("--output", required=True)
    return parser.parse_args()


def run() -> None:
    args = parse_args()
    import cv2  # Imported after argument parsing so setup diagnostics stay clear.

    emit_progress("opening", 0.0, "Video yerel yüz algılama motorunda açılıyor.")
    capture = cv2.VideoCapture(args.source)
    if not capture.isOpened():
        raise RuntimeError("OpenCV could not open the selected video")
    source_width = int(round(capture.get(cv2.CAP_PROP_FRAME_WIDTH)))
    source_height = int(round(capture.get(cv2.CAP_PROP_FRAME_HEIGHT)))
    source_fps = float(capture.get(cv2.CAP_PROP_FPS))
    if source_width <= 0 or source_height <= 0:
        capture.release()
        raise RuntimeError("OpenCV reported invalid source dimensions")

    cascade_path = Path(cv2.data.haarcascades) / "haarcascade_frontalface_default.xml"
    cascade = cv2.CascadeClassifier(os.fspath(cascade_path))
    if cascade.empty():
        capture.release()
        raise RuntimeError("OpenCV Haar face cascade could not be loaded")

    interval_ms = max(250, int(args.interval_ms))
    sample_times = list(range(0, max(1, args.duration_ms), interval_ms))
    total = len(sample_times)
    clusters: list[FaceCluster] = []
    active: FaceCluster | None = None
    pending_times: list[float] = []
    samples: list[dict[str, Any]] = []
    warnings: list[str] = []
    hit_frames = 0
    decode_failures = 0
    activation_hits = MIN_ACTIVATION_HITS if args.duration_ms >= 30_000 else 2

    for index, relative_ms in enumerate(sample_times):
        capture.set(cv2.CAP_PROP_POS_MSEC, float(args.start_ms + relative_ms))
        ok, frame = capture.read()
        if not ok or frame is None:
            decode_failures += 1
            if active is not None:
                samples.append(
                    {"t": relative_ms, "b": expand_face_to_panel(active.box), "c": HELD_CONFIDENCE}
                )
            else:
                pending_times.append(relative_ms)
            continue
        frame = resize_for_analysis(cv2, frame, args.max_dimension)
        faces = detect_faces(cv2, cascade, frame)
        if faces:
            hit_frames += 1
        matched_active = False
        for face in faces:
            matched = next((cluster for cluster in clusters if cluster.matches(face)), None)
            if matched is None:
                if len(clusters) >= MAX_CLUSTERS:
                    clusters.sort(key=lambda cluster: cluster.hit_times[-1] if cluster.hit_times else 0.0)
                    clusters.pop(0)
                clusters.append(FaceCluster(face, relative_ms))
            else:
                matched.absorb(face, relative_ms)
                if matched is active:
                    matched_active = True

        # Promote or switch the active cluster with hysteresis so a momentary
        # second face cannot steal the layout from a stable facecam.
        best = None
        best_recent = 0
        for cluster in clusters:
            recent = cluster.recent_hits(relative_ms)
            if recent > best_recent or (
                recent == best_recent and best is not None and cluster.total_hits > best.total_hits
            ):
                best = cluster
                best_recent = recent
        if best is not None and best is not active and best_recent >= activation_hits:
            active_recent = active.recent_hits(relative_ms) if active is not None else 0
            if active is None or best_recent >= active_recent + SWITCH_HYSTERESIS_HITS or active_recent == 0:
                if active is not None:
                    warnings.append(
                        "Yayıncı kamerası video içinde yer değiştirdi; yeni konum otomatik izlendi."
                    )
                active = best

        if active is not None:
            if pending_times:
                backfill_box = expand_face_to_panel(active.box)
                for pending in pending_times:
                    samples.append({"t": pending, "b": backfill_box, "c": BACKFILL_CONFIDENCE})
                pending_times.clear()
            confidence = 1.0 if matched_active else HELD_CONFIDENCE
            samples.append(
                {"t": relative_ms, "b": expand_face_to_panel(active.box), "c": confidence}
            )
        else:
            pending_times.append(relative_ms)

        if index % 5 == 0 or index == total - 1:
            emit_progress(
                "facecam-scan",
                2.0 + 96.0 * (index + 1) / max(1, total),
                f"Yayıncı kamerası aranıyor · kare {index + 1}/{total}",
            )

    capture.release()

    if active is None or not samples:
        raise RuntimeError(
            "FACECAM_NOT_FOUND: Videoda sabit bir yayıncı kamerası yüzü bulunamadı"
        )
    if decode_failures > total // 4:
        warnings.append(
            "Videonun bir bölümü çözülemedi; facecam konumu mevcut karelerden çıkarıldı."
        )
    if hit_frames < max(2, total // 20):
        warnings.append(
            "Yüz az sayıda karede algılandı; facecam kırpımı düşük güvenle üretildi."
        )

    samples.sort(key=lambda sample: sample["t"])
    deduplicated: dict[int, dict[str, Any]] = {}
    for sample in samples:
        timestamp = max(0, min(args.duration_ms - 1, int(round(sample["t"]))))
        sample["t"] = timestamp
        previous = deduplicated.get(timestamp)
        if previous is None or sample["c"] > previous["c"]:
            deduplicated[timestamp] = sample

    result = {
        "schema": SCHEMA,
        "requestKey": args.request_key,
        "engine": ENGINE,
        "engineVersion": f"{cv2.__version__};cascade=frontalface_default",
        "sourceWidth": source_width,
        "sourceHeight": source_height,
        "sourceFps": source_fps if math.isfinite(source_fps) and source_fps > 0 else None,
        "confidenceMetric": CONFIDENCE_METRIC,
        "samples": list(deduplicated.values()),
        "warnings": list(dict.fromkeys(warnings)),
    }
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(result, handle, ensure_ascii=False, separators=(",", ":"), allow_nan=False)
        handle.write("\n")
        handle.flush()
        os.fsync(handle.fileno())
    emit_progress("complete", 100.0, "Yayıncı kamerası algılama tamamlandı.")


if __name__ == "__main__":
    try:
        run()
    except Exception as error:  # The Rust host turns this into a structured diagnostic.
        print(f"AUTO_REFRAME_ERROR {type(error).__name__}: {error}", file=sys.stderr, flush=True)
        raise SystemExit(1)
