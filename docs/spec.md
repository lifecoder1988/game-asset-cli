# CLI Spec

## Product Scope

`game-asset-cli` generates and transforms game resources:

- Raster images: scenes, backgrounds, UI, icons, characters, sprites, logo, VFX.
- Transparent assets: crop reference regions, generate green-screen sources, key
  green to alpha PNG.
- Animation assets: sprite sheets, frame sequences, video-to-frame extraction.
- Audio: BGM through MiniMax, local synthetic SFX, audio normalization,
  trimming, looping, conversion, waveform previews.
- Audit artifacts: contact sheets, waveform sheets, manifests.

The CLI must stay usable inside shell scripts and CI. A user should be able to
delete every generated file and reproduce the same workflow by re-running the
same commands or the same batch spec.

## Stateless Contract

The CLI must not:

- Provide `init`.
- Create or own a project.
- Persist hidden state in the workspace or home directory.
- Start a daemon, runner, job queue, local API server, or background worker.
- Store API keys, auth tokens, job IDs, sessions, or provider responses outside
  explicitly requested output files.
- Require a manifest to run a single command.

The CLI may:

- Read provider auth from environment variables.
- Use temporary files during a command, then clean them up.
- Write a metadata sidecar only when the user passes `--metadata-out`.
- Read a batch YAML file when the user calls `batch`.
- Produce a manifest when the user explicitly calls `manifest`.

Temporary files are implementation details. They must be placed under the OS temp
directory or next to the target output with a unique suffix, and removed or
atomically renamed before the command exits.

## Global Options

Every command follows the same IO policy:

```text
--out <path>          required for single-file generators
--out-dir <path>      required for multi-file generators
--overwrite           replace existing output
--dry-run             validate inputs and print planned provider calls
--json                emit JSONL events to stdout
--metadata-out <path> write explicit JSON metadata
--quiet               suppress non-error human logs
```

Default overwrite behavior is fail-closed: if an output path exists and
`--overwrite` is not set, the command exits without modifying it.

## Command Surface

### Image Generation

```bash
game-asset image generate \
  --kind scene|background|character|sprite|ui|icon|logo|effect|frames|map|tile \
  --prompt prompt.md \
  --ref key_art.png \
  --size 1280x720 \
  --out art/scenes/gameplay.png
```

Rules:

- Backend: `codex-image` by default.
- `--prompt` reads a file; `--prompt-text` is allowed for small one-off prompts.
- `--ref` is repeatable and passed to Codex as reference images.
- `--style` may point to a text style anchor; it is merged into the generated
  Codex prompt.
- `--size` is a requested canvas constraint. The backend must validate the final
  output dimensions and warn when the provider does not honor them.

### Reference Crop

```bash
game-asset image crop \
  --in art/scenes/gameplay.png \
  --box 120,80,240,96 \
  --out refs/btn_play_crop.png
```

`--box` is pixel coordinates: `x,y,w,h`. A future `--box-percent` may accept
normalized scene coordinates.

### Green Source

```bash
game-asset image green-source \
  --kind button|panel|icon|bar|character|prop|effect \
  --prompt prompts/btn_play.md \
  --ref refs/btn_play_crop.png \
  --out assets/source/btn_play_green.png
```

Rules:

- Backend: `codex-image`.
- The generated image must use a pure green `#00FF00` background unless
  `--key-color` overrides it.
- The command is for one asset at a time. Multi-asset sheets belong to
  `sheet-pack`, not `green-source`.

### Chroma Key

```bash
game-asset image chroma-key \
  --in assets/source/btn_play_green.png \
  --out assets/ui/btn_play.png \
  --key "#00ff00" \
  --tolerance 42 \
  --despill 0.75 \
  --feather 1.0 \
  --trim
```

This command is pure local image processing. It must not call Codex or any remote
API.

### Sprite and Frame Tools

```bash
game-asset sprite sheet-slice --in hero_run.png --grid 8x1 --out-dir frames/hero_run
game-asset sprite sheet-pack --in-dir frames/hero_run --cols 8 --out hero_run_sheet.png --metadata-out hero_run.json
game-asset sprite normalize --in hero.png --size 512x512 --fit contain --anchor center --out hero_norm.png
```

### Video Slice

```bash
game-asset video slice \
  --in source/hit_fx.mp4 \
  --start 0.4 \
  --end 1.6 \
  --frames 12 \
  --key auto \
  --out-dir fx/hit/frames
```

`video slice` can shell out to `ffmpeg` first. The command must fail with a clear
dependency error when `ffmpeg` is missing.

### Background Music

```bash
game-asset audio bgm \
  --prompt prompts/menu_bgm.md \
  --instrumental \
  --model music-2.6 \
  --format mp3 \
  --sample-rate 44100 \
  --bitrate 256000 \
  --out audio/bgm/menu.mp3
```

Rules:

- Backend: `minimax-music`.
- `MINIMAX_API_KEY` is read from the environment by default.
- `--target-duration` may be accepted as a prompt hint and post-processing goal,
  but it is not a hard MiniMax API parameter unless the provider API adds one.
- For game BGM, `--instrumental` should be the default unless lyrics are supplied.
- If vocals are requested without explicit lyrics, the command should require
  `--lyrics-optimizer` or fail before calling MiniMax.

### Local Sound Effects

```bash
game-asset audio sfx \
  --preset coin|click|confirm|cancel|hit|explosion|jump|laser|powerup|error|whoosh \
  --duration-ms 260 \
  --pitch 880 \
  --variation 3 \
  --out audio/sfx/coin.wav
```

Rules:

- Backend: `local-sfx` by default.
- Generation must be deterministic for the same options and seed.
- `--variations N --out-dir <dir>` creates numbered alternatives.
- A future model backend must keep this command shape and only change
  `--backend`.

### Audio Post-processing

```bash
game-asset audio trim --in raw.wav --start 0.1 --end 1.8 --out trimmed.wav
game-asset audio normalize --in raw.wav --target-lufs -16 --out normalized.wav
game-asset audio loop --in bgm.mp3 --crossfade-ms 1200 --out bgm_loop.wav
game-asset audio convert --in bgm.wav --format ogg --out bgm.ogg
game-asset audio waveform --in bgm.wav --out audit/bgm_waveform.png
game-asset audio sprite --in-dir audio/sfx --out audio/sfx_sprite.ogg --metadata-out audio/sfx_sprite.json
```

### Audit and Manifest

```bash
game-asset contact-sheet --in "assets/ui/*.png" --out audit/ui_contact.png
game-asset manifest --in dist/assets --out dist/assets.manifest.json
game-asset doctor
```

`doctor` checks local dependencies and provider credentials. It must not write
state.

### Batch

```bash
game-asset batch --spec assets.yaml --parallel 4
```

Batch mode reads a versioned input spec and executes the listed tasks. It may
resolve dependencies in memory for that invocation, but it must not persist a job
database.

## Exit Codes

- `0`: success.
- `1`: general command failure.
- `2`: invalid arguments or invalid spec.
- `3`: input file missing or invalid.
- `4`: output exists and `--overwrite` is not set.
- `5`: provider dependency missing or provider auth missing.
- `6`: provider returned an error or failed to produce the requested artifact.
- `7`: post-processing validation failed.

## JSONL Events

With `--json`, commands emit machine-readable events:

```json
{"type":"start","command":"audio.bgm","out":"audio/bgm/menu.mp3"}
{"type":"provider_request","provider":"minimax-music","model":"music-2.6"}
{"type":"artifact","path":"audio/bgm/menu.mp3","kind":"audio","bytes":813651}
{"type":"done","elapsed_ms":18420}
```

No event is required to include secrets or full prompt text. Prompt text should be
included only with `--json-include-prompts`.
