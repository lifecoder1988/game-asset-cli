---
name: game-asset
description: >-
  Generate and post-process 2D game assets with the `game-asset` CLI: AI image
  generation via Codex (concept art, characters, sprites, UI, icons, logos,
  backgrounds, tiles, effects), green-screen/chroma-key isolation, sprite sheet
  slice/pack/normalize, background music (MiniMax) and deterministic sound
  effects, video frame extraction, contact sheets, and manifests. Use when asked
  to create or process game art, sprites, icons, tilesets, UI, BGM/music, or
  sound effects, or to crop/normalize/pack/slice/chroma-key game assets.
---

# game-asset

`game-asset` is a stateless CLI for generating and post-processing 2D game
assets. Every command reads explicit inputs and writes explicit outputs — no
project state, no daemon, no hidden files. The binary is `game-asset`.

Run `game-asset doctor` first in a new environment to confirm dependencies.

## Conventions (apply to every command)

- Outputs never overwrite by default. Pass `--overwrite` to replace an existing
  file, or the command fails with `output exists: ...`.
- Parent directories of `--out` are created automatically.
- Global flags (before or after the subcommand): `-v`/`--verbose` (diagnostics
  to stderr), `--json` (machine-readable event stream on stdout), `--quiet`,
  `--json-include-prompts`.
- On failure the process exits non-zero; with `--json` it emits a single
  `{"type":"error",...}` object.

## Image generation (Codex-backed) — the main feature

Generates one image by driving the local **Codex** CLI's built-in image tool.

```bash
# Prompt from a file (best for long/structured prompts):
CODEX_SANDBOX=danger-full-access \
game-asset image generate \
  --kind concept \
  --prompt prompts/key_art.md \
  --size 1536x864 \
  --out art/key_art.png \
  --metadata-out art/key_art.metadata.json \
  --overwrite -v

# Or inline text:
game-asset image generate --kind character --prompt-text "a stout dwarf blacksmith, idle pose" --out hero.png
```

Flags: `--kind <K>` (required), `--prompt <file>` or `--prompt-text <str>`
(required, pick one), `--style <file>` (extra style constraints), `--ref <img>`
(reference image, repeatable), `--size WxH`, `--out <file>` (required),
`--metadata-out <file>`, `--codex-model <name>`, `--timeout-seconds <n>`
(default 300), `--overwrite`, `--dry-run`.

`--kind` values: `scene concept background character sprite ui icon logo effect
frames map tile`.

Notes:
- You do **not** need to put `$imagegen` in your prompt; the tool prepends it.
- `--dry-run` (with `-v`) prints the exact instruction without calling Codex —
  use it to preview the prompt.
- Codex writes the PNG into `~/.codex/generated_images/<uuid>/`; the tool finds
  the freshly-generated file (by launch time), validates it (PNG signature,
  size, decodes, non-blank), and copies it to `--out`.

### Required environment for image generation

- **`CODEX_SANDBOX`** — `workspace-write` (default), `read-only`, or
  `danger-full-access`. Use `danger-full-access` if generation hangs/fails under
  the default sandbox.
- **`CODEX_REASONING_EFFORT`** — defaults to `high`. The tool forces this because
  with effort `none` the model fabricates a fake image path instead of actually
  generating. Leave it at `high` unless you have a reason to lower it.
- **`CODEX_BIN`** — optional path to the Codex executable (default: `codex`).
- Codex must be authenticated (`codex login`) **and have working image
  generation** — `$imagegen` is handled by codex's `imagegen` system skill, whose
  built-in `image_gen` path saves into `~/.codex/generated_images/`. If that
  backend is unauthorized, codex narrates success but writes no file and the run
  times out; fix codex's auth/image access (see Troubleshooting).

### Green-screen asset (for clean cutouts)

Generates one asset on a flat chroma-key background, ready for `chroma-key`.

```bash
game-asset image green-source --kind button --prompt prompts/btn.md --out btn_green.png
game-asset image chroma-key --in btn_green.png --out btn.png --trim --overwrite
```

`green-source` `--kind` values: `button panel icon bar character prop effect`.
Shares the Codex flags above plus `--key-color "#00ff00"`.

## Image post-processing (local, no Codex)

```bash
# Crop a rectangle (x,y,w,h):
game-asset image crop --in scene.png --box 120,80,240,96 --out crop.png

# Remove a chroma-key background (auto color from corners, or pass --key):
game-asset image chroma-key --in green.png --out out.png \
  --key "#00ff00" --tolerance 42 --despill 0.75 --feather 0 --trim
```

## Sprites

```bash
# Resize+anchor onto a fixed canvas:
game-asset sprite normalize --in hero.png --size 512x512 --out hero_norm.png \
  --fit contain --anchor center --trim

# Slice a sheet into frames (cols x rows):
game-asset sprite sheet-slice --in sheet.png --grid 8x1 --out-dir frames

# Pack frames back into a sheet (+ JSON atlas):
game-asset sprite sheet-pack --in-dir frames --cols 8 --out sheet.png --metadata-out sheet.json
```

`--fit`: `contain cover stretch`. `--anchor`: `center top-left bottom-center`.

## Audio

```bash
# Background music via MiniMax (needs MINIMAX_API_KEY):
game-asset audio bgm --prompt bgm.md --instrumental --out bgm.mp3
# (lyrics: --lyrics <file>/--lyrics-text; --lyrics-optimizer; --format mp3;
#  --sample-rate 44100; --bitrate 256000; --model music-2.6-free)

# Deterministic local SFX (no network):
game-asset audio sfx --preset coin --duration-ms 260 --out coin.wav
# Variations: --variations 5 --out-dir sfx/ (also --pitch, --seed, --sample-rate)

# Trim / waveform (PCM WAV in: 8/16/24/32-bit int or 32-bit float; not mp3):
game-asset audio trim --in raw.wav --start 0.1 --end 1.8 --out trimmed.wav
game-asset audio waveform --in coin.wav --out coin_wave.png --width 1200 --height 240
```

`--preset` values: `click confirm cancel coin powerup error hit explosion jump
laser whoosh`.

## Video, audit, self-update

```bash
# Extract evenly-sampled PNG frames (requires ffmpeg on PATH):
game-asset video slice --in fx.mp4 --start 0.4 --end 1.6 --frames 12 --out-dir fx/hit

# Contact sheet from a glob; JSON manifest of a directory:
game-asset contact-sheet --in "assets/*.png" --out contact.png --cols 6 --cell 160
game-asset manifest --in dist/assets --out assets.manifest.json

# Dependency/credential check; self-update:
game-asset doctor
game-asset upgrade            # latest; or --check / --tag vX.Y.Z / --dry-run / --force
```

## Worked example: concept → scene → asset → transparent cutout

The core authoring flow. Each generate step feeds the previous output in as a
`--ref` so style/identity stays consistent. Keep the env exports for the whole
session.

```bash
export CODEX_SANDBOX=danger-full-access   # use if default sandbox stalls
# CODEX_REASONING_EFFORT defaults to high (leave it; "none" makes Codex fabricate)
mkdir -p art prompts
```

### 1. Generate a concept image

The prompt describes the overall art direction (mood, palette, subject).

```bash
game-asset image generate \
  --kind concept \
  --prompt prompts/concept.md \
  --size 1536x864 \
  --out art/concept.png \
  --metadata-out art/concept.metadata.json -v
```

### 2. Generate a scene, referencing the concept

Pass the concept as `--ref` so the scene inherits its style. The scene prompt
describes the specific layout/composition.

```bash
game-asset image generate \
  --kind scene \
  --prompt prompts/scene.md \
  --ref art/concept.png \
  --size 1536x864 \
  --out art/scene.png -v
```

### 3. Generate a raw asset on a solid background, referencing a slice

First crop the element you want out of the scene to use as a visual reference
(the "slice"), then `green-source` produces that asset alone on a flat
chroma-key background. Use `--ref` so it matches the scene's rendition.

```bash
# Crop the slice (x,y,w,h) from the scene:
game-asset image crop --in art/scene.png --box 640,300,256,256 --out art/slice.png

# Generate the isolated asset on a solid green background, referencing the slice:
game-asset image green-source \
  --kind prop \
  --prompt prompts/asset.md \
  --ref art/slice.png \
  --key-color "#00ff00" \
  --out art/asset_green.png -v
```

### 4. Key out the solid background → transparent PNG

`chroma-key` removes the flat background; `--trim` crops to the asset's bounds.

```bash
game-asset image chroma-key \
  --in art/asset_green.png \
  --out art/asset.png \
  --key "#00ff00" --trim --overwrite
```

`art/asset.png` is now a transparent, tightly-cropped game asset. From here you
can `sprite normalize` it onto a fixed canvas, or `sprite sheet-pack` several
frames into a sheet (see the Sprites section).

> `--ref` is repeatable — pass multiple references (e.g. `--ref concept.png
> --ref slice.png`) when an asset should honor more than one source.

### Prompt templates for the steps above

Plain text/markdown; the tool prepends the `$imagegen` trigger. Keep one
coherent art direction across all three so references stay consistent. End image
prompts with negatives like "no text, no watermark, no UI".

`prompts/concept.md` — overall art direction:

```text
Concept art for a 2D side-scrolling fantasy adventure game. An enchanted forest
at dawn: warm golden light through a misty canopy, painterly hand-drawn style,
saturated teal-and-amber palette, soft rim light. Mood: serene and adventurous.
No characters, no text, no watermark, no UI.
```

`prompts/scene.md` — a specific playable background in that style:

```text
A full game background in the established concept style: a mossy stone clearing
with a glowing ancient shrine on the right, twisted roots in the foreground, a
distant waterfall on the left. Layered depth for parallax, cinematic
composition, an open center for gameplay. No characters, no text, no watermark.
```

`prompts/asset.md` — one isolated prop matching the reference slice:

```text
A single game prop matching the reference image: an ornate glowing shrine
lantern, 3/4 view, crisp clean silhouette, lighting consistent with the
reference. Exactly one object, centered, on a flat solid background. No ground
shadow, no scene, no text, no watermark.
```

## Sprite-sheet pipeline (after cutouts)

1. `game-asset sprite normalize --in art/asset.png --size 256x256 --out frames/f0.png --trim`
2. `game-asset sprite sheet-pack --in-dir frames --cols 8 --out sheet.png --metadata-out sheet.json`

## Troubleshooting image generation

- **"codex did not generate an image in generated_images" / time out** — usually
  a Codex-side problem, not the CLI. Re-run with `-v` and read the streamed log.
- **Codex narrates success ("Generated …") but no file appears** — codex's
  built-in `image_gen` backend is unauthorized/broken: the model reports success
  while writing nothing. Confirm by running bare codex and searching for a fresh
  PNG (`find ~ /tmp -name '*.png' -mmin -5`). Fix codex's image-gen access:
  re-`codex login`; check the account/plan has image generation; watch for
  `auth fail:token fail` / `AuthorizationRequired` / HTTP 403 in the `-v` log.
  As a backend-independent fallback, the `imagegen` system skill can run
  `~/.codex/skills/.system/imagegen/scripts/image_gen.py` with `OPENAI_API_KEY`.
- **`reasoning effort: none` in the banner** — the tool forces
  `model_reasoning_effort=high`; don't override it lower (low effort makes the
  model fabricate instead of generating).
- **Sandbox blocks the write** — try `CODEX_SANDBOX=danger-full-access`.
- Use `-v` to see the full instruction, the exact `codex exec …` command, the
  live Codex log, and `selected generated image: …`. Use `--dry-run -v` to
  preview the prompt without spending a generation.
