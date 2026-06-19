# Provider Design

This document defines how external generators plug into the stateless CLI.

## Provider Rules

- A provider receives a fully resolved request and an explicit output target.
- A provider may use temporary files during one command.
- A provider must not write durable state.
- A provider must return only after the requested artifact exists locally.
- Provider metadata is written only when `--metadata-out` is provided.
- Provider replacement must not change the public command shape.

## `codex-image`

Purpose: generate raster game images through the local Codex CLI.

Supported asset kinds:

- `scene`
- `background`
- `character`
- `sprite`
- `ui`
- `icon`
- `logo`
- `effect`
- `frames`
- `map`
- `tile`

Runtime:

```text
codex exec \
  --ephemeral \
  --skip-git-repo-check \
  --sandbox workspace-write \
  -C <workdir> \
  <generated instruction> \
  --image <ref1> \
  --image <ref2>
```

Use the installed CLI's current option names. At the time this doc was written,
local `codex exec --help` exposes `--ephemeral`, `--skip-git-repo-check`,
`--sandbox`, `-C/--cd`, `--image`, `--json`, and `--output-last-message`.

Configuration:

- `CODEX_BIN`: path to the Codex executable. Default: `codex`.
- `CODEX_SANDBOX`: default sandbox mode. Default: `workspace-write`.
- `--codex-model`: optional model override passed to `codex exec --model`.

Prompt construction:

- The Rust CLI owns prompt templates per `--kind`.
- User prompt text is inserted as the subject brief.
- `--style` text is inserted as reusable style constraints.
- `--ref` files are passed as Codex images and mentioned in the instruction.
- For `green-source`, the template must explicitly require a pure `#00FF00`
  background and one isolated asset.

Validation:

- Output file exists.
- File decodes as a supported image.
- Expected alpha/background constraints are checked when applicable.
- Optional size check compares decoded dimensions to `--size`.

Failure modes:

- `codex` missing: exit code `5`.
- Codex exits non-zero: exit code `6`.
- Codex exits zero but does not write the file: exit code `6`.
- Output is not a valid image: exit code `7`.

## `minimax-music`

Purpose: generate game BGM and longer musical beds.

Official API facts checked on 2026-06-19:

- MiniMax lists music generation in its API overview.
- Current model docs list `Music-2.6` and legacy `Music-2.0`.
- The Music Generation API is `POST https://api.minimax.io/v1/music_generation`.
- The request uses bearer auth and `application/json`.
- Model options include `music-2.6`, `music-cover`, `music-2.6-free`, and
  `music-cover-free`.
- The API accepts `prompt`, `lyrics`, `audio_setting`, `lyrics_optimizer`,
  `is_instrumental`, and cover-reference fields.
- `output_format` supports `hex` and `url`; URL links expire after 24 hours.

CLI mapping:

```bash
game-asset audio bgm \
  --prompt prompts/menu.md \
  --instrumental \
  --model music-2.6 \
  --format mp3 \
  --sample-rate 44100 \
  --bitrate 256000 \
  --out audio/menu.mp3
```

Request mapping:

```json
{
  "model": "music-2.6",
  "prompt": "<prompt text>",
  "lyrics": "<optional lyrics>",
  "lyrics_optimizer": false,
  "is_instrumental": true,
  "output_format": "hex",
  "audio_setting": {
    "sample_rate": 44100,
    "bitrate": 256000,
    "format": "mp3"
  }
}
```

Defaults:

- `--model music-2.6-free` for development unless the user selects paid model.
- `--instrumental true` for BGM when no lyrics are supplied.
- `output_format: "hex"` to avoid expiring URL handling.
- `format: mp3`, `sample_rate: 44100`, `bitrate: 256000`.

Notes:

- Do not expose duration as a guaranteed provider parameter unless MiniMax adds a
  documented request field. Use `--target-duration` as prompt text and optional
  local post-processing only.
- For `output_format: "url"`, download the result immediately and write the local
  output file before exiting.
- Preserve `trace_id`, `base_resp`, and `extra_info` in metadata when requested.

## `local-sfx`

Purpose: generate short game sound effects locally without a model call.

Supported presets:

- `click`
- `confirm`
- `cancel`
- `coin`
- `powerup`
- `error`
- `hit`
- `explosion`
- `jump`
- `laser`
- `whoosh`

CLI mapping:

```bash
game-asset audio sfx \
  --preset coin \
  --duration-ms 260 \
  --pitch 880 \
  --seed 42 \
  --out audio/sfx/coin.wav
```

Output:

- WAV is required for v1.
- OGG/MP3 can be supported through `audio convert`.
- Same inputs and seed must produce byte-identical WAV output.

Future model switch:

```bash
game-asset audio sfx --backend model --preset coin --prompt "bright coin pickup" --out coin.wav
```

The command stays `audio sfx`; only the backend changes.

## Local Post-processors

These are not providers because they do not generate semantic content:

- `image crop`
- `image chroma-key`
- `sprite sheet-slice`
- `sprite sheet-pack`
- `sprite normalize`
- `video slice`
- `audio trim`
- `audio normalize`
- `audio loop`
- `audio convert`
- `audio waveform`
- `contact-sheet`
- `manifest`

They should be pure local transformations.

