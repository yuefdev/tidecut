# Third-party notices

## RNNoise neural weights

Astral Lunar's optional **AI Ses Temizleme** feature downloads the pinned
`beguiling-drafter-2018-08-30/bd.rnnn` weights from
[`GregorR/rnnoise-models`](https://github.com/GregorR/rnnoise-models).

- Runtime name: `rnnoise-bd-v1`
- Pinned commit: `3eee541a283fd3b8f81b85b1748e3b9ccbefa04d`
- Upstream file: `beguiling-drafter-2018-08-30/bd.rnnn`
- SHA-256: `ae3f7411e1e6a884f839a4a145c394408398f09854dbc1216ee02faafc98a17b`
- Upstream notice: the repository states that, except for its tools and
  README, the trained-model work is not subject to copyright.

The model is processed locally by FFmpeg's `arnndn` recurrent-neural-network
filter. The original media is never overwritten.

## Silero VAD

Voice activity detection for Voice Rider, silence suggestions, and music
ducking uses the embedded `silero_vad.onnx` model from
[`snakers4/silero-vad`](https://github.com/snakers4/silero-vad), released under
the MIT License. The full license text and pinned model checksum are kept in
`src-tauri/models/SILERO_VAD_LICENSE.txt` and `src-tauri/models/README.md`.

## Beat This! beat and downbeat model

Beat analysis uses the MIT-licensed Rust port
[`beat-this-rs`](https://github.com/danigb/beat-this-rs) 1.0.0 at commit
`089b509247e6fdcec666511c0dcf0d5f39c21e73` and the MIT-licensed published
weights from the original JKU/CPJKU
[`Beat This!`](https://github.com/CPJKU/beat_this) project.

- Mel front end SHA-256: `fdd59e65c515331308e4c8841edf99972deca646bdf6197744c2a5b7755e3de9`
- Full beat model SHA-256: `5f810debe53459b559127fb55bbad40035bb47cc567b20e501670f968c770f02`

The upstream authors note that some source training files are copyrighted or
carry limited Creative Commons terms and leave downstream use assessment to
integrators. Astral Lunar distributes neither those training files nor the
training datasets; it downloads only the separately published MIT model
weights after checksum verification.

## Whisper and whisper.cpp

Turkish transcript suggestions use the official Windows x64 binary from
[`ggml-org/whisper.cpp`](https://github.com/ggml-org/whisper.cpp) v1.9.1 and a
multilingual GGML Whisper model converted and published by the project's
official model repository. Whisper code and model weights, and whisper.cpp,
are released under the MIT License.

- `whisper-bin-x64.zip` SHA-256: `7d8be46ecd31828e1eb7a2ecdd0d6b314feafd82163038ab6092594b0a063539`
- `ggml-small-q5_1.bin` SHA-256: `ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb`

Both artifacts are downloaded on first use, verified before installation, and
run locally. Audio and transcripts are not uploaded.

## OpenCV CSRT auto reframe

The optional **AI Kadraj** feature installs pinned Python wheels on first use
and runs object tracking entirely on the local machine. Video frames and
tracking coordinates are not uploaded.

- `opencv-contrib-python-headless` 4.10.0.84 — Apache License 2.0
- `numpy` 1.26.4 — BSD 3-Clause License
- `uv` 0.11.28 — Apache License 2.0 or MIT License

The OpenCV package supplies the CSRT tracker. NumPy is used only by the local
tracker runner. `uv` creates the isolated Python 3.11 runtime used by this
optional feature.

## MediaPipe YAMNet laughter detection

**Akıllı Shorts** can install an isolated local Python runtime and download the
official float32 MediaPipe YAMNet Audio Classifier model. The app sends no
audio to Google: FFmpeg creates temporary 16 kHz mono PCM, MediaPipe performs
inference locally, and the temporary PCM is removed after analysis.

- `mediapipe` 0.10.35 — Apache License 2.0
- YAMNet / TensorFlow Models — Apache License 2.0
- Pinned model URL: `mediapipe-models/audio_classifier/yamnet/float32/1/yamnet.tflite`
- Model size: `4,126,810` bytes
- Model SHA-256: `4d8b4a53282dc83ef04e3e7dbc4fbc98082e34e44ed798e16c3a0cdd4c584faf`
- `numpy` 1.26.4 — BSD 3-Clause License
- `opencv-contrib-python-headless` 4.10.0.84 — Apache License 2.0
- `matplotlib` 3.11.1 — PSF-based Matplotlib License

YAMNet predicts 521 AudioSet event classes and was trained from the
AudioSet-YouTube corpus. Astral Lunar distributes neither the training corpus
nor source YouTube recordings; it downloads only the checksum-verified model
artifact and uses the laughter-family class scores.

## FFmpeg

Media processing uses an FFmpeg runtime downloaded by `ffmpeg-sidecar`.
FFmpeg licensing depends on the configuration of the distributed binary; see
the [FFmpeg legal page](https://ffmpeg.org/legal.html) and the runtime's
`ffmpeg -version` configuration output.
