# Batch Spec

Batch mode is planned and documented here for design alignment. It is not
implemented in the current `game-asset` binary.

Batch mode lets users run repeatable multi-asset jobs without turning the CLI
into a project manager.

The spec file is explicit input. It is not CLI state.

## Invocation

```bash
game-asset batch --spec assets.yaml --parallel 4
```

## Schema Overview

```yaml
version: 1

defaults:
  overwrite: false
  image:
    backend: codex-image
    size: 1024x1024
  music:
    backend: minimax-music
    model: music-2.6-free
    format: mp3
    sample_rate: 44100
    bitrate: 256000
  sfx:
    backend: local-sfx
    format: wav

tasks:
  - id: key_art
    type: image.generate
    kind: scene
    prompt: prompts/key_art.md
    out: dist/art/key_art.png

  - id: gameplay_scene
    type: image.generate
    kind: scene
    prompt: prompts/gameplay_scene.md
    refs:
      - dist/art/key_art.png
    out: dist/art/scenes/gameplay.png
    needs: [key_art]

  - id: play_button_crop
    type: image.crop
    in: dist/art/scenes/gameplay.png
    box: [120, 80, 240, 96]
    out: dist/refs/play_button_crop.png
    needs: [gameplay_scene]

  - id: play_button_green
    type: image.green-source
    kind: button
    prompt: prompts/play_button.md
    refs:
      - dist/refs/play_button_crop.png
    out: dist/source/play_button_green.png
    needs: [play_button_crop]

  - id: play_button
    type: image.chroma-key
    in: dist/source/play_button_green.png
    key: "#00ff00"
    trim: true
    out: dist/ui/play_button.png
    needs: [play_button_green]

  - id: menu_bgm
    type: audio.bgm
    prompt: prompts/menu_bgm.md
    instrumental: true
    target_duration: 60s
    out: dist/audio/bgm/menu.mp3

  - id: coin_sfx
    type: audio.sfx
    preset: coin
    duration_ms: 260
    pitch: 880
    variations: 4
    out_dir: dist/audio/sfx/coin
```

## Common Task Fields

- `id`: unique task ID.
- `type`: command type.
- `needs`: optional list of task IDs that must finish first.
- `out`: single output file.
- `out_dir`: output directory for multi-artifact tasks.
- `overwrite`: overrides global default.
- `metadata_out`: optional JSON metadata path.

No task may rely on hidden generated state. Dependencies are only ordering
constraints and path references.

## Supported Task Types

- `image.generate`
- `image.crop`
- `image.green-source`
- `image.chroma-key`
- `sprite.sheet-slice`
- `sprite.sheet-pack`
- `sprite.normalize`
- `video.slice`
- `audio.bgm`
- `audio.sfx`
- `audio.trim`
- `audio.normalize`
- `audio.loop`
- `audio.convert`
- `audio.waveform`
- `contact-sheet`
- `manifest`

## Execution Rules

1. Parse and validate the full spec.
2. Confirm all referenced input files exist unless they are outputs of `needs`.
3. Confirm no output collision unless overwrite is enabled.
4. Build an in-memory DAG from `needs`.
5. Execute ready tasks up to `--parallel`.
6. Stop on first failure unless `--keep-going` is set.
7. Return non-zero if any task fails.

## Path Rules

Paths are resolved relative to the batch spec file's directory unless they are
absolute.

Globs are allowed only for audit commands:

- `contact-sheet`
- `manifest`
- future `audio.contact-sheet`

Generation tasks must use concrete paths so outputs are predictable.

## Environment

Batch mode does not define credentials. Providers read the same environment as
single commands:

- `CODEX_BIN`
- `CODEX_SANDBOX`
- `MINIMAX_API_KEY`

## JSONL Events

With `--json`, each task event includes the batch task ID:

```json
{"type":"task_start","id":"menu_bgm","task_type":"audio.bgm"}
{"type":"provider_request","id":"menu_bgm","provider":"minimax-music","model":"music-2.6-free"}
{"type":"artifact","id":"menu_bgm","path":"dist/audio/bgm/menu.mp3","kind":"audio"}
{"type":"task_done","id":"menu_bgm","elapsed_ms":21044}
```
