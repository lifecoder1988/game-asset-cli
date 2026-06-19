# Technical Design

## Implementation Shape

Rust is the primary implementation language. Start as one binary crate, but keep
module boundaries compatible with a future workspace split.

Suggested module layout:

```text
src/
  main.rs
  cli/                 # clap command definitions and argument validation
  core/                # request/response structs, output policy, errors
  providers/
    codex_image.rs
    minimax_music.rs
    local_sfx.rs
  media/
    image.rs           # crop, chroma key, trim, contact sheet
    audio.rs           # WAV IO, normalize, trim, loop, waveform
    video.rs           # ffmpeg wrapper
    sprite.rs          # sheet slice/pack/metadata
  batch/
    spec.rs            # YAML schema
    executor.rs
  manifest.rs
  events.rs            # human logs and JSONL events
```

Primary crate choices:

- CLI: `clap`.
- Serialization: `serde`, `serde_json`, `serde_yaml`.
- Errors: `thiserror`, `anyhow` only at the binary boundary.
- Async HTTP/process orchestration: `tokio`, `reqwest`.
- Image IO and processing: `image`, plus small in-house alpha/keying routines.
- WAV generation: `hound` for local SFX. Add encoders behind feature flags only
  when needed.
- Logging/events: `tracing` for internal logs, explicit JSONL writer for CLI
  events.

Do not add a database, embedded KV store, background worker framework, or config
home directory.

## Core Types

Provider modules should depend on typed requests, not raw CLI args.

```rust
pub struct OutputTarget {
    pub path: PathBuf,
    pub overwrite: bool,
    pub metadata_out: Option<PathBuf>,
}

pub struct ImageGenerateRequest {
    pub kind: ImageKind,
    pub prompt: String,
    pub refs: Vec<PathBuf>,
    pub size: Option<(u32, u32)>,
    pub output: OutputTarget,
}

pub struct MusicGenerateRequest {
    pub prompt: String,
    pub lyrics: Option<String>,
    pub instrumental: bool,
    pub model: String,
    pub format: AudioFormat,
    pub sample_rate: Option<u32>,
    pub bitrate: Option<u32>,
    pub output: OutputTarget,
}

pub struct SfxGenerateRequest {
    pub preset: SfxPreset,
    pub duration_ms: u32,
    pub pitch_hz: Option<f32>,
    pub seed: u64,
    pub output: OutputTarget,
}
```

Provider traits:

```rust
#[async_trait::async_trait]
pub trait ImageGenerator {
    async fn generate(&self, req: ImageGenerateRequest) -> Result<Artifact>;
}

#[async_trait::async_trait]
pub trait MusicGenerator {
    async fn generate_bgm(&self, req: MusicGenerateRequest) -> Result<Artifact>;
}

pub trait SfxGenerator {
    fn generate_sfx(&self, req: SfxGenerateRequest) -> Result<Artifact>;
}
```

The command layer selects providers and then validates the output artifact.

## IO and Atomicity

All file-producing commands use this flow:

1. Validate inputs.
2. Refuse to overwrite existing outputs unless `--overwrite` is set.
3. Create output parent directories.
4. Write to a temporary sibling path.
5. Validate the temp file.
6. Atomically rename temp to final path.
7. Write optional metadata sidecar.

Provider commands that must write directly to a path, such as Codex image
generation, should write into a temporary output path first and then let the Rust
wrapper validate and rename it.

## Codex Process Boundary

The Codex backend is a subprocess adapter, not a library dependency.

The wrapper should:

- Resolve `CODEX_BIN`, defaulting to `codex`.
- Run `codex exec` non-interactively.
- Prefer `--ephemeral` so Codex does not persist a session for this generation.
- Use `--skip-git-repo-check` because asset generation often runs outside a Git
  repository.
- Use `--sandbox workspace-write` by default.
- Set `-C` to a temporary work directory or the output parent.
- Attach references with `--image`.
- Instruct Codex to write exactly one file at the requested relative output path.
- Verify that the requested file exists and is a valid image.

The CLI must not parse Codex session state. The only successful outcome is a
valid output file.

## MiniMax HTTP Boundary

The MiniMax music provider is an HTTP adapter.

The wrapper should:

- Read `MINIMAX_API_KEY` from the environment unless overridden.
- Send `Authorization: Bearer <key>`.
- Use `POST https://api.minimax.io/v1/music_generation`.
- Prefer `output_format: "hex"` so the CLI can decode the response directly and
  not depend on expiring URLs.
- Support `output_format: "url"` only when the implementation downloads the file
  immediately before exiting.
- Preserve `trace_id` and `extra_info` in `--metadata-out` when requested.

API errors are provider failures, not retries forever. Implement bounded retries
only for transient network failures and rate-limit responses.

## Local SFX Engine

Local SFX should be deterministic and fast. Implement it as small DSP building
blocks:

- Oscillators: sine, square, triangle, saw, noise.
- Envelopes: ADSR, exponential decay, pitch envelope.
- Filters: one-pole low/high-pass, simple band-pass.
- Effects: soft clip, short delay, optional convolution-free reverb.
- Rendering: 44.1 kHz or 48 kHz WAV, mono by default.

Presets are parameterized graphs. For example:

- `coin`: sine arpeggio + fast attack + decay + light saturation.
- `click`: short noise burst + high-pass + click transient.
- `hit`: noise + low sine thump + fast decay.
- `explosion`: filtered noise + descending low oscillator.
- `whoosh`: noise sweep with band-pass movement.

When a model SFX backend is added later, it implements the same request/response
contract and the same command remains valid.

## Batch Executor

Batch execution is still stateless:

- The YAML file is an input.
- Dependency resolution happens in memory.
- Outputs are files requested in the YAML.
- No job database is written.

Executor behavior:

- Validate the whole spec before starting.
- Build a DAG from `needs`.
- Run independent tasks up to `--parallel`.
- Stop on first failure by default.
- `--keep-going` continues independent tasks after a failure.
- Emit JSONL events with task IDs.

## Security and Secrets

- Never print API keys.
- Redact provider headers in debug logs.
- Do not include full prompt text in JSONL unless explicitly requested.
- Do not read provider credentials from config files owned by this CLI.
- Allow Codex to use its own existing auth, but do not copy or inspect Codex auth
  files.
- Treat input prompts and reference images as untrusted file paths. Canonicalize
  paths before passing them to subprocesses.

## Test Strategy

Unit tests:

- CLI argument validation.
- YAML spec parsing.
- Output overwrite policy.
- Chroma key edge cases.
- Crop bounds.
- Local SFX deterministic snapshots.

Integration tests:

- Stub Codex backend writes a fixture PNG.
- Stub MiniMax server returns hex audio.
- `ffmpeg` missing dependency error.
- Batch dependency ordering.

Golden artifacts:

- Small PNG fixtures for crop/keying.
- Small WAV fixtures for waveform, trim, normalize.
- JSONL event snapshots.

Provider live tests should be opt-in through environment variables:

```bash
RUN_LIVE_CODEX=1 cargo test live_codex
RUN_LIVE_MINIMAX=1 MINIMAX_API_KEY=... cargo test live_minimax
```

