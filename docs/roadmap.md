# Roadmap

## M0 - Spec and Architecture

- Document stateless product boundary.
- Define command surface.
- Define Rust module layout.
- Define provider adapters for Codex image, MiniMax BGM, local SFX.
- Define batch YAML format.

## M1 - Rust Skeleton and Local Media Tools

- Create Rust binary crate.
- Add `clap` command tree.
- Implement output policy and JSONL event writer.
- Implement local commands:
  - `image crop`
  - `image chroma-key`
  - `sprite normalize`
  - `audio sfx`
  - `audio trim`
  - `audio waveform`
  - `doctor`

## M2 - Codex Image Backend

- Implement `codex-image` subprocess adapter.
- Add prompt templates per image kind.
- Support repeated `--ref`.
- Validate output image existence and dimensions.
- Add live test gated by `RUN_LIVE_CODEX=1`.

## M3 - MiniMax BGM Backend

- Implement `minimax-music` HTTP adapter.
- Decode hex audio output to local file.
- Support immediate download for URL output.
- Add metadata sidecar support.
- Add live test gated by `RUN_LIVE_MINIMAX=1`.

## M4 - Batch and Manifest

- Implement `batch --spec`.
- Add task DAG execution and `--parallel`.
- Add `manifest`.
- Add `contact-sheet`.

## M5 - Sprite, Video, and Packaging Utilities

- Implement `sprite sheet-slice`.
- Implement `sprite sheet-pack`.
- Implement `video slice` through `ffmpeg`.
- Add audio sprite generation.

## M6 - Model-backed SFX

- Add a remote SFX provider behind `audio sfx --backend model`.
- Keep local SFX as the default for deterministic UI and arcade effects.
- Reuse the same CLI shape and batch task schema.

## Non-goals

- No `init`.
- No project model.
- No persistent state.
- No login or account management.
- No database.
- No OSS storage.
- No runner daemon.
- No async job queue.
- No human confirmation state.
- No game build or gamedev pipeline.

