# 002: whisper.cpp for local speech to text

Status: accepted
Date: 2026-09-30
Relates to: v0.7.0 polish and dx, issue 37, v0.0.1 proof of concept, v0.4.0 performance pass, v0.5.0 Intel aware work

## Context

Susurro promises dictation that works without internet on low and mid range CPU only hardware. The reference machine uses an 8 core 10th gen Intel CPU, integrated graphics only, and 16 GB RAM. Local transcription acts as the guarantee, while cloud providers act as an optional boost when online. The local engine must run base.en from v0.0.1, keep CI light, report actionable errors, and leave a path to GPU acceleration later.

## Decision

The project uses whisper.cpp through an external `whisper-cli` binary for local speech to text. `adapters-stt-local/src/lib.rs` implements `WhisperLocal` behind `SpeechToTextPort`. It writes 16 kHz mono S16 audio to a temp WAV file, runs the binary with model, language, no print, no timestamp, and no speech threshold flags, reads stdout as transcript, and rejects blank output such as empty strings and bracketed tags. Missing models and missing binaries produce actionable messages that name the model path, the env override, and the doctor command.

Three helpers extend the base. `WindowedPartial` decodes at most a trailing 8 second window every 3 seconds for live display, and the final full decode decides the transcript. `MockStt` returns fixed text for hardware free tests and CI. `adapters-stt-local/src/openvino.rs` detects OpenVINO readiness through binary flag, iGPU node, and runtime probes, resolves auto, cpu, and openvino requests to a concrete backend, and appends the encoder device flag only for the OpenVINO path. The decoder stays on CPU, and the backend description states placement per run.

## Consequences

Dictation works offline from the first milestone with a small model and clear errors. CI stays light, because tests use the mock and shape tests assert on command construction without running the binary. Streaming feedback stays bounded in CPU, because partials decode a fixed window on a cadence. The Intel path sets honest expectations, because detection names the missing piece and falls back to CPU silently in auto mode and loudly in explicit mode.

The choice pays process spawn and WAV file cost per decode. Native binding work remains future work after v0.5.0. Partial hypotheses can differ from final text, so the UI must mark partials as display only and settle them on final decode.

## Alternatives

The project rejected cloud only transcription, because it breaks the offline promise and fails on the reference hardware use case. The project rejected Vosk, because accuracy and English model quality lagged the whisper family for dictation at the time. The project rejected an immediate native whisper binding, because it raised build complexity before the capture to injection loop proved out. The project rejected ONNX Runtime as the default in this milestone, because the runner did not exist yet, so the code detects the name only and benchmarks the proven CPU and OpenVINO paths.

## Links

* `adapters-stt-local/src/lib.rs` implements `WhisperLocal`, `WindowedPartial`, `MockStt`, and the WAV encoder
* `adapters-stt-local/src/openvino.rs` implements backend detection, resolve policy, and stored winner handling
* `core/src/ports.rs` defines `SpeechToTextPort`, `Transcript`, and the partial contract
* `core/src/state.rs` defines the transcribing state that wraps local decode
* `susurro-project-plan.md` v0.0.1, v0.4.0, and v0.5.0 sections
