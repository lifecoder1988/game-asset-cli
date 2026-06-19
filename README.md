# game-asset-cli

Stateless CLI for generating and post-processing game assets.

The tool does not create projects, keep job state, start a daemon, or write
hidden state. Every command reads explicit inputs and writes explicit outputs.

## Build

```bash
cargo build
```

The binary name is `game-asset`.

## Implemented Commands

Image and sprite utilities:

```bash
game-asset image generate --kind scene --prompt prompt.md --ref key.png --out scene.png
game-asset image green-source --kind button --prompt prompt.md --ref crop.png --out button_green.png
game-asset image crop --in scene.png --box 120,80,240,96 --out crop.png
game-asset image chroma-key --in button_green.png --out button.png --trim
game-asset sprite normalize --in hero.png --size 512x512 --out hero_norm.png
game-asset sprite sheet-slice --in hero_sheet.png --grid 8x1 --out-dir frames
game-asset sprite sheet-pack --in-dir frames --cols 8 --out hero_sheet.png --metadata-out hero_sheet.json
```

Audio utilities:

```bash
game-asset audio bgm --prompt bgm.md --instrumental --out bgm.mp3
game-asset audio sfx --preset coin --duration-ms 260 --out coin.wav
game-asset audio waveform --in coin.wav --out coin_waveform.png
game-asset audio trim --in raw.wav --start 0.1 --end 1.8 --out trimmed.wav
```

Audit and metadata:

```bash
game-asset contact-sheet --in "assets/*.png" --out contact.png
game-asset manifest --in dist/assets --out assets.manifest.json
game-asset doctor
```

Video slicing:

```bash
game-asset video slice --in effect.mp4 --start 0.4 --end 1.6 --frames 12 --out-dir fx/hit
```

`video slice` requires `ffmpeg` on `PATH`.

## Providers

Image generation wraps the local Codex CLI:

- `CODEX_BIN` optionally points to the Codex executable.
- `CODEX_SANDBOX` defaults to `workspace-write`.

Background music uses the MiniMax music API:

- `MINIMAX_API_KEY` must be set for `audio bgm`.

Short SFX are generated locally and deterministically.

## Docs

See [docs/](docs/) for the product spec, technical design, provider design, and
roadmap.

