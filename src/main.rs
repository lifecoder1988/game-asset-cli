use clap::{Args, Parser, Subcommand, ValueEnum};
use image::{imageops, DynamicImage, Rgba, RgbaImage};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    env,
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
    time::{Instant, SystemTime},
};
use tempfile::TempDir;
use tokio::{
    process::Command as TokioCommand,
    time::{sleep, Duration},
};

#[derive(Parser)]
#[command(name = "game-asset")]
#[command(version)]
#[command(about = "Stateless game asset generation and post-processing CLI")]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[arg(long, global = true)]
    quiet: bool,
    #[arg(long = "json-include-prompts", global = true)]
    json_include_prompts: bool,
    /// Print verbose generation logs to stderr (resolved command, full
    /// provider instruction, and the provider's own streamed output).
    #[arg(short = 'v', long, global = true)]
    verbose: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Generate and transform raster images")]
    Image(ImageCmd),
    #[command(about = "Normalize, slice, and pack sprite assets")]
    Sprite(SpriteCmd),
    #[command(about = "Extract frame sequences from video")]
    Video(VideoCmd),
    #[command(about = "Generate and process audio assets")]
    Audio(AudioCmd),
    #[command(about = "Create an image contact sheet from input globs")]
    ContactSheet(ContactSheetArgs),
    #[command(about = "Write a JSON manifest for an asset directory")]
    Manifest(ManifestArgs),
    #[command(about = "Check local provider dependencies and credentials")]
    Doctor,
    #[command(about = "Self-update to the latest released binary")]
    Upgrade(UpgradeArgs),
}

#[derive(Args)]
struct UpgradeArgs {
    /// Only report whether a newer version exists; do not download or install.
    #[arg(long)]
    check: bool,
    /// Install this exact release tag instead of the latest (e.g. v0.2.0).
    #[arg(long)]
    tag: Option<String>,
    /// GitHub repository to fetch releases from, as owner/name.
    #[arg(long, default_value = "lifecoder1988/game-asset-cli")]
    repo: String,
    /// Reinstall even when already on the target version.
    #[arg(long)]
    force: bool,
    /// Resolve the release and platform asset but do not download or replace the binary.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Args)]
struct ImageCmd {
    #[command(subcommand)]
    command: ImageSubcommand,
}

#[derive(Subcommand)]
enum ImageSubcommand {
    #[command(about = "Generate one image through the Codex image provider")]
    Generate(ImageGenerateArgs),
    #[command(about = "Crop a rectangle from an image")]
    Crop(CropArgs),
    #[command(name = "green-source")]
    #[command(about = "Generate one asset on a chroma-key background")]
    GreenSource(GreenSourceArgs),
    #[command(name = "chroma-key")]
    #[command(about = "Remove a chroma-key background from an image")]
    ChromaKey(ChromaKeyArgs),
}

#[derive(Args)]
struct ImageGenerateArgs {
    #[arg(long, value_enum)]
    kind: ImageKind,
    #[arg(long)]
    prompt: Option<PathBuf>,
    #[arg(long = "prompt-text")]
    prompt_text: Option<String>,
    #[arg(long)]
    style: Option<PathBuf>,
    #[arg(long = "ref")]
    refs: Vec<PathBuf>,
    #[arg(long)]
    size: Option<String>,
    #[arg(long)]
    out: PathBuf,
    #[arg(long)]
    overwrite: bool,
    #[arg(long = "metadata-out")]
    metadata_out: Option<PathBuf>,
    #[arg(long = "codex-model")]
    codex_model: Option<String>,
    #[arg(long = "timeout-seconds", default_value_t = 300)]
    timeout_seconds: u64,
    #[arg(long)]
    dry_run: bool,
}

#[derive(Args)]
struct GreenSourceArgs {
    #[arg(long, value_enum)]
    kind: GreenKind,
    #[arg(long)]
    prompt: Option<PathBuf>,
    #[arg(long = "prompt-text")]
    prompt_text: Option<String>,
    #[arg(long = "ref")]
    refs: Vec<PathBuf>,
    #[arg(long = "key-color", default_value = "#00ff00")]
    key_color: String,
    #[arg(long)]
    out: PathBuf,
    #[arg(long)]
    overwrite: bool,
    #[arg(long = "metadata-out")]
    metadata_out: Option<PathBuf>,
    #[arg(long = "codex-model")]
    codex_model: Option<String>,
    #[arg(long = "timeout-seconds", default_value_t = 300)]
    timeout_seconds: u64,
    #[arg(long)]
    dry_run: bool,
}

#[derive(Args)]
struct CropArgs {
    #[arg(long = "in")]
    input: PathBuf,
    #[arg(long)]
    box_: String,
    #[arg(long)]
    out: PathBuf,
    #[arg(long)]
    overwrite: bool,
}

#[derive(Args)]
struct ChromaKeyArgs {
    #[arg(long = "in")]
    input: PathBuf,
    #[arg(long)]
    out: PathBuf,
    #[arg(long, default_value = "#00ff00")]
    key: String,
    #[arg(long, default_value_t = 42.0)]
    tolerance: f32,
    #[arg(long, default_value_t = 0.75)]
    despill: f32,
    #[arg(long, default_value_t = 0.0)]
    feather: f32,
    #[arg(long)]
    trim: bool,
    #[arg(long)]
    overwrite: bool,
}

#[derive(Args)]
struct SpriteCmd {
    #[command(subcommand)]
    command: SpriteSubcommand,
}

#[derive(Subcommand)]
enum SpriteSubcommand {
    #[command(name = "sheet-slice")]
    #[command(about = "Slice a sprite sheet into frame PNGs")]
    SheetSlice(SheetSliceArgs),
    #[command(name = "sheet-pack")]
    #[command(about = "Pack frame PNGs into a sprite sheet")]
    SheetPack(SheetPackArgs),
    #[command(about = "Resize and anchor a sprite onto a fixed canvas")]
    Normalize(SpriteNormalizeArgs),
}

#[derive(Args)]
struct SheetSliceArgs {
    #[arg(long = "in")]
    input: PathBuf,
    #[arg(long)]
    grid: String,
    #[arg(long = "out-dir")]
    out_dir: PathBuf,
    #[arg(long)]
    overwrite: bool,
}

#[derive(Args)]
struct SheetPackArgs {
    #[arg(long = "in-dir")]
    input_dir: PathBuf,
    #[arg(long)]
    cols: u32,
    #[arg(long)]
    out: PathBuf,
    #[arg(long = "metadata-out")]
    metadata_out: Option<PathBuf>,
    #[arg(long)]
    overwrite: bool,
}

#[derive(Args)]
struct SpriteNormalizeArgs {
    #[arg(long = "in")]
    input: PathBuf,
    #[arg(long)]
    out: PathBuf,
    #[arg(long)]
    size: String,
    #[arg(long, value_enum, default_value_t = FitMode::Contain)]
    fit: FitMode,
    #[arg(long, value_enum, default_value_t = Anchor::Center)]
    anchor: Anchor,
    #[arg(long)]
    trim: bool,
    #[arg(long)]
    overwrite: bool,
}

#[derive(Args)]
struct VideoCmd {
    #[command(subcommand)]
    command: VideoSubcommand,
}

#[derive(Subcommand)]
enum VideoSubcommand {
    #[command(about = "Extract evenly sampled PNG frames from a video")]
    Slice(VideoSliceArgs),
}

#[derive(Args)]
struct VideoSliceArgs {
    #[arg(long = "in")]
    input: PathBuf,
    #[arg(long)]
    start: f32,
    #[arg(long)]
    end: f32,
    #[arg(long, default_value_t = 12)]
    frames: u32,
    #[arg(long)]
    key: Option<String>,
    #[arg(long = "out-dir")]
    out_dir: PathBuf,
    #[arg(long)]
    overwrite: bool,
}

#[derive(Args)]
struct AudioCmd {
    #[command(subcommand)]
    command: AudioSubcommand,
}

#[derive(Subcommand)]
enum AudioSubcommand {
    #[command(about = "Generate background music with MiniMax")]
    Bgm(AudioBgmArgs),
    #[command(about = "Generate deterministic local sound effects")]
    Sfx(AudioSfxArgs),
    #[command(about = "Trim a WAV file by time range")]
    Trim(AudioTrimArgs),
    #[command(about = "Render a waveform PNG from a WAV file")]
    Waveform(AudioWaveformArgs),
}

#[derive(Args)]
struct AudioBgmArgs {
    #[arg(long)]
    prompt: Option<PathBuf>,
    #[arg(long = "prompt-text")]
    prompt_text: Option<String>,
    #[arg(long)]
    lyrics: Option<PathBuf>,
    #[arg(long = "lyrics-text")]
    lyrics_text: Option<String>,
    #[arg(long)]
    instrumental: bool,
    #[arg(long = "lyrics-optimizer")]
    lyrics_optimizer: bool,
    #[arg(long, default_value = "music-2.6-free")]
    model: String,
    #[arg(long, default_value = "mp3")]
    format: String,
    #[arg(long = "sample-rate", default_value_t = 44100)]
    sample_rate: u32,
    #[arg(long, default_value_t = 256000)]
    bitrate: u32,
    #[arg(long)]
    out: PathBuf,
    #[arg(long)]
    overwrite: bool,
    #[arg(long = "metadata-out")]
    metadata_out: Option<PathBuf>,
    #[arg(long)]
    dry_run: bool,
}

#[derive(Args)]
struct AudioSfxArgs {
    #[arg(long, value_enum)]
    preset: SfxPreset,
    #[arg(long = "duration-ms", default_value_t = 260)]
    duration_ms: u32,
    #[arg(long)]
    pitch: Option<f32>,
    #[arg(long, default_value_t = 0)]
    seed: u64,
    #[arg(long, default_value_t = 44100)]
    sample_rate: u32,
    #[arg(long)]
    out: Option<PathBuf>,
    #[arg(long = "out-dir")]
    out_dir: Option<PathBuf>,
    #[arg(long, default_value_t = 1)]
    variations: u32,
    #[arg(long)]
    overwrite: bool,
}

#[derive(Args)]
struct AudioTrimArgs {
    #[arg(long = "in")]
    input: PathBuf,
    #[arg(long)]
    start: f32,
    #[arg(long)]
    end: f32,
    #[arg(long)]
    out: PathBuf,
    #[arg(long)]
    overwrite: bool,
}

#[derive(Args)]
struct AudioWaveformArgs {
    #[arg(long = "in")]
    input: PathBuf,
    #[arg(long)]
    out: PathBuf,
    #[arg(long, default_value_t = 1200)]
    width: u32,
    #[arg(long, default_value_t = 240)]
    height: u32,
    #[arg(long)]
    overwrite: bool,
}

#[derive(Args)]
struct ContactSheetArgs {
    #[arg(long = "in")]
    inputs: Vec<String>,
    #[arg(long)]
    out: PathBuf,
    #[arg(long, default_value_t = 6)]
    cols: u32,
    #[arg(long, default_value_t = 160)]
    cell: u32,
    #[arg(long)]
    overwrite: bool,
}

#[derive(Args)]
struct ManifestArgs {
    #[arg(long = "in")]
    input: PathBuf,
    #[arg(long)]
    out: PathBuf,
    #[arg(long)]
    overwrite: bool,
}

#[derive(Clone, ValueEnum)]
enum ImageKind {
    Scene,
    Concept,
    Background,
    Character,
    Sprite,
    Ui,
    Icon,
    Logo,
    Effect,
    Frames,
    Map,
    Tile,
}

#[derive(Clone, ValueEnum)]
enum GreenKind {
    Button,
    Panel,
    Icon,
    Bar,
    Character,
    Prop,
    Effect,
}

#[derive(Clone, ValueEnum)]
enum FitMode {
    Contain,
    Cover,
    Stretch,
}

#[derive(Clone, ValueEnum)]
enum Anchor {
    Center,
    TopLeft,
    BottomCenter,
}

enum KeyMode {
    Auto,
    Fixed([u8; 3]),
}

#[derive(Clone, Copy, ValueEnum)]
enum SfxPreset {
    Click,
    Confirm,
    Cancel,
    Coin,
    Powerup,
    Error,
    Hit,
    Explosion,
    Jump,
    Laser,
    Whoosh,
}

#[derive(Debug)]
struct CliError {
    code: i32,
    message: String,
}

type Result<T> = std::result::Result<T, CliError>;

impl CliError {
    fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl From<io::Error> for CliError {
    fn from(err: io::Error) -> Self {
        CliError::new(1, err.to_string())
    }
}

#[tokio::main]
async fn main() {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            // clap handles its own --help/--version/usage output. But when the
            // caller asked for --json, a parse failure must still honor the
            // documented contract: a single {"type":"error",...} object on stdout
            // rather than clap's plain-text usage on stderr.
            use clap::error::ErrorKind;
            let wants_json = env::args().skip(1).any(|a| a == "--json");
            let display_only = matches!(
                err.kind(),
                ErrorKind::DisplayHelp
                    | ErrorKind::DisplayVersion
                    | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            );
            if wants_json && !display_only {
                let code = err.exit_code();
                println!(
                    "{}",
                    json!({"type": "error", "code": code, "message": err.to_string()})
                );
                std::process::exit(code);
            }
            err.exit();
        }
    };
    let json = cli.json;
    let started = Instant::now();
    let code = match run(cli, started).await {
        Ok(()) => 0,
        Err(err) => {
            if json {
                println!(
                    "{}",
                    json!({"type": "error", "code": err.code, "message": err.message})
                );
            } else {
                eprintln!("error: {}", err.message);
            }
            err.code
        }
    };
    std::process::exit(code);
}

async fn run(cli: Cli, started: Instant) -> Result<()> {
    let ctx = Ctx {
        json: cli.json,
        quiet: cli.quiet,
        include_prompts: cli.json_include_prompts,
        verbose: cli.verbose,
        pending_start: Mutex::new(None),
    };
    let (command_name, out) = command_label(&cli.command);
    let mut start_event = serde_json::Map::new();
    start_event.insert("command".into(), Value::String(command_name));
    if let Some(out) = out {
        start_event.insert("out".into(), Value::String(out));
    }
    ctx.event("start", Value::Object(start_event));
    match cli.command {
        Commands::Image(cmd) => match cmd.command {
            ImageSubcommand::Generate(args) => image_generate(&ctx, args).await?,
            ImageSubcommand::Crop(args) => image_crop(&ctx, args)?,
            ImageSubcommand::GreenSource(args) => image_green_source(&ctx, args).await?,
            ImageSubcommand::ChromaKey(args) => image_chroma_key(&ctx, args)?,
        },
        Commands::Sprite(cmd) => match cmd.command {
            SpriteSubcommand::SheetSlice(args) => sprite_sheet_slice(&ctx, args)?,
            SpriteSubcommand::SheetPack(args) => sprite_sheet_pack(&ctx, args)?,
            SpriteSubcommand::Normalize(args) => sprite_normalize(&ctx, args)?,
        },
        Commands::Video(cmd) => match cmd.command {
            VideoSubcommand::Slice(args) => video_slice(&ctx, args)?,
        },
        Commands::Audio(cmd) => match cmd.command {
            AudioSubcommand::Bgm(args) => audio_bgm(&ctx, args).await?,
            AudioSubcommand::Sfx(args) => audio_sfx(&ctx, args)?,
            AudioSubcommand::Trim(args) => audio_trim(&ctx, args)?,
            AudioSubcommand::Waveform(args) => audio_waveform(&ctx, args)?,
        },
        Commands::ContactSheet(args) => contact_sheet(&ctx, args)?,
        Commands::Manifest(args) => manifest(&ctx, args)?,
        Commands::Doctor => doctor(&ctx)?,
        Commands::Upgrade(args) => upgrade(&ctx, args).await?,
    }
    ctx.event("done", json!({"elapsed_ms": started.elapsed().as_millis()}));
    Ok(())
}

struct Ctx {
    json: bool,
    quiet: bool,
    include_prompts: bool,
    verbose: bool,
    // The `start` event is buffered here and only flushed once the first real
    // event (provider_request/artifact/warning/done) is emitted. A command that
    // fails during validation never produces a real event, so on the error path
    // the start is dropped and only a single `error` object reaches stdout,
    // satisfying the documented JSON contract.
    pending_start: Mutex<Option<Value>>,
}

fn command_label(command: &Commands) -> (String, Option<String>) {
    let display = |p: &Path| p.display().to_string();
    match command {
        Commands::Image(cmd) => match &cmd.command {
            ImageSubcommand::Generate(a) => ("image.generate".into(), Some(display(&a.out))),
            ImageSubcommand::Crop(a) => ("image.crop".into(), Some(display(&a.out))),
            ImageSubcommand::GreenSource(a) => ("image.green-source".into(), Some(display(&a.out))),
            ImageSubcommand::ChromaKey(a) => ("image.chroma-key".into(), Some(display(&a.out))),
        },
        Commands::Sprite(cmd) => match &cmd.command {
            SpriteSubcommand::SheetSlice(a) => {
                ("sprite.sheet-slice".into(), Some(display(&a.out_dir)))
            }
            SpriteSubcommand::SheetPack(a) => ("sprite.sheet-pack".into(), Some(display(&a.out))),
            SpriteSubcommand::Normalize(a) => ("sprite.normalize".into(), Some(display(&a.out))),
        },
        Commands::Video(cmd) => match &cmd.command {
            VideoSubcommand::Slice(a) => ("video.slice".into(), Some(display(&a.out_dir))),
        },
        Commands::Audio(cmd) => match &cmd.command {
            AudioSubcommand::Bgm(a) => ("audio.bgm".into(), Some(display(&a.out))),
            AudioSubcommand::Sfx(a) => (
                "audio.sfx".into(),
                a.out.as_deref().or(a.out_dir.as_deref()).map(display),
            ),
            AudioSubcommand::Trim(a) => ("audio.trim".into(), Some(display(&a.out))),
            AudioSubcommand::Waveform(a) => ("audio.waveform".into(), Some(display(&a.out))),
        },
        Commands::ContactSheet(a) => ("contact-sheet".into(), Some(display(&a.out))),
        Commands::Manifest(a) => ("manifest".into(), Some(display(&a.out))),
        Commands::Doctor => ("doctor".into(), None),
        Commands::Upgrade(_) => ("upgrade".into(), None),
    }
}

impl Ctx {
    /// Emit a diagnostic line to stderr when `--verbose` is set. Independent of
    /// the stdout JSON contract (always stderr) and of `--quiet` (verbose is an
    /// explicit opt-in), so it never corrupts machine-readable output.
    fn vlog(&self, msg: impl AsRef<str>) {
        if self.verbose {
            eprintln!("[verbose] {}", msg.as_ref());
        }
    }

    fn event(&self, typ: &str, value: Value) {
        if typ == "start" {
            *self.pending_start.lock().unwrap() = Some(value);
            return;
        }
        if let Some(start) = self.pending_start.lock().unwrap().take() {
            self.emit("start", start);
        }
        self.emit(typ, value);
    }

    fn emit(&self, typ: &str, value: Value) {
        if self.json {
            let mut obj = serde_json::Map::new();
            obj.insert("type".into(), Value::String(typ.into()));
            if let Value::Object(map) = value {
                for (k, v) in map {
                    obj.insert(k, v);
                }
            }
            println!("{}", Value::Object(obj));
        } else if !self.quiet {
            match typ {
                "artifact" => {
                    if let Some(path) = value.get("path").and_then(Value::as_str) {
                        eprintln!("wrote {path}");
                    }
                }
                "provider_request" => {
                    if let Some(provider) = value.get("provider").and_then(Value::as_str) {
                        if value
                            .get("dry_run")
                            .and_then(Value::as_bool)
                            .unwrap_or(false)
                        {
                            eprintln!("planned provider request: {provider}");
                        } else if let Some(timeout) =
                            value.get("timeout_seconds").and_then(Value::as_u64)
                        {
                            eprintln!("running {provider} (timeout {timeout}s)");
                        } else {
                            eprintln!("running {provider}");
                        }
                    }
                }
                "warning" => {
                    if let Some(message) = value.get("message").and_then(Value::as_str) {
                        eprintln!("warning: {message}");
                    }
                }
                _ => {}
            }
        }
    }
}

async fn image_generate(ctx: &Ctx, args: ImageGenerateArgs) -> Result<()> {
    check_output_available(&args.out, args.overwrite)?;
    validate_timeout(args.timeout_seconds)?;
    let prompt = read_prompt(args.prompt.as_deref(), args.prompt_text.as_deref())?;
    let style = read_optional(args.style.as_deref())?;
    let size = args.size.as_deref().map(parse_size).transpose()?;
    let instruction = image_instruction(
        image_kind_name(&args.kind),
        &prompt,
        style.as_deref(),
        size,
        false,
        "#00FF00",
        &args.out,
    );
    run_codex_image(
        ctx,
        &instruction,
        &args.refs,
        &args.out,
        args.overwrite,
        args.metadata_out.as_deref(),
        args.codex_model.as_deref(),
        size,
        None,
        args.timeout_seconds,
        args.dry_run,
    )
    .await
}

async fn image_green_source(ctx: &Ctx, args: GreenSourceArgs) -> Result<()> {
    check_output_available(&args.out, args.overwrite)?;
    validate_timeout(args.timeout_seconds)?;
    let key = parse_hex_color(&args.key_color)?;
    let prompt = read_prompt(args.prompt.as_deref(), args.prompt_text.as_deref())?;
    let instruction = image_instruction(
        green_kind_name(&args.kind),
        &prompt,
        None,
        None,
        true,
        &args.key_color,
        &args.out,
    );
    run_codex_image(
        ctx,
        &instruction,
        &args.refs,
        &args.out,
        args.overwrite,
        args.metadata_out.as_deref(),
        args.codex_model.as_deref(),
        None,
        Some(key),
        args.timeout_seconds,
        args.dry_run,
    )
    .await
}

fn image_crop(ctx: &Ctx, args: CropArgs) -> Result<()> {
    ensure_output(&args.out, args.overwrite)?;
    let (x, y, w, h) = parse_box(&args.box_)?;
    let img =
        image::open(&args.input).map_err(|e| CliError::new(3, format!("decode image: {e}")))?;
    let max_x = x
        .checked_add(w)
        .ok_or_else(|| CliError::new(2, "crop box exceeds image bounds"))?;
    let max_y = y
        .checked_add(h)
        .ok_or_else(|| CliError::new(2, "crop box exceeds image bounds"))?;
    if max_x > img.width() || max_y > img.height() {
        return Err(CliError::new(2, "crop box exceeds image bounds"));
    }
    let cropped = img.crop_imm(x, y, w, h);
    save_image_atomic(&args.out, &cropped, args.overwrite)?;
    ctx.event("artifact", json!({"path": args.out, "kind": "image"}));
    Ok(())
}

fn image_chroma_key(ctx: &Ctx, args: ChromaKeyArgs) -> Result<()> {
    ensure_output(&args.out, args.overwrite)?;
    let key = parse_hex_color(&args.key)?;
    // Reject non-finite/negative numerics up front. Without this, --tolerance NaN
    // keys nothing yet still drives despill (band=NaN) and silently discolors the
    // image, and a negative tolerance is a silent no-op masking a user typo.
    validate_nonnegative_f32("--tolerance", args.tolerance)?;
    validate_nonnegative_f32("--despill", args.despill)?;
    validate_nonnegative_f32("--feather", args.feather)?;
    warn_if_alpha_dropping_format(ctx, &args.out);
    let img =
        image::open(&args.input).map_err(|e| CliError::new(3, format!("decode image: {e}")))?;
    let mut rgba = img.to_rgba8();
    let spill = args.despill.clamp(0.0, 1.0);
    let feather = args.feather.max(0.0);
    for pixel in rgba.pixels_mut() {
        let d = color_distance(pixel.0, key);
        // Alpha coverage: 0 inside the key tolerance, ramping up to 1 across the
        // feather band just outside it (hard edge when --feather is 0).
        let coverage = if d <= args.tolerance {
            0.0
        } else if feather > 0.0 {
            ((d - args.tolerance) / feather).clamp(0.0, 1.0)
        } else {
            1.0
        };
        if coverage <= 0.0 {
            pixel.0[3] = 0;
        } else {
            apply_despill(&mut pixel.0, key, d, args.tolerance, spill);
            pixel.0[3] = (pixel.0[3] as f32 * coverage).round() as u8;
        }
    }
    let out = if args.trim {
        trim_alpha(&rgba).unwrap_or(rgba)
    } else {
        rgba
    };
    save_rgba_atomic(&args.out, &out, args.overwrite)?;
    ctx.event("artifact", json!({"path": args.out, "kind": "image"}));
    Ok(())
}

fn sprite_normalize(ctx: &Ctx, args: SpriteNormalizeArgs) -> Result<()> {
    ensure_output(&args.out, args.overwrite)?;
    warn_if_alpha_dropping_format(ctx, &args.out);
    let (tw, th) = parse_size(&args.size)?;
    let img =
        image::open(&args.input).map_err(|e| CliError::new(3, format!("decode image: {e}")))?;
    let src = if args.trim {
        DynamicImage::ImageRgba8(trim_alpha(&img.to_rgba8()).unwrap_or_else(|| img.to_rgba8()))
    } else {
        img
    };
    let resized = match args.fit {
        FitMode::Stretch => src.resize_exact(tw, th, imageops::FilterType::Lanczos3),
        FitMode::Contain => src.resize(tw, th, imageops::FilterType::Lanczos3),
        FitMode::Cover => src.resize_to_fill(tw, th, imageops::FilterType::Lanczos3),
    };
    let mut canvas = RgbaImage::from_pixel(tw, th, Rgba([0, 0, 0, 0]));
    let (x, y) = anchor_offset(args.anchor, tw, th, resized.width(), resized.height());
    imageops::overlay(&mut canvas, &resized.to_rgba8(), x.into(), y.into());
    save_rgba_atomic(&args.out, &canvas, args.overwrite)?;
    ctx.event("artifact", json!({"path": args.out, "kind": "image"}));
    Ok(())
}

fn sprite_sheet_slice(ctx: &Ctx, args: SheetSliceArgs) -> Result<()> {
    let (cols, rows) = parse_grid(&args.grid)?;
    if args.out_dir.exists() && !args.overwrite {
        return Err(CliError::new(
            4,
            format!("output directory exists: {}", args.out_dir.display()),
        ));
    }
    // Decode and validate the grid BEFORE touching the filesystem so an invalid
    // grid (e.g. larger than the image) doesn't leave a stray empty out-dir.
    let img = image::open(&args.input)
        .map_err(|e| CliError::new(3, format!("decode image: {e}")))?
        .to_rgba8();
    let cell_w = img.width() / cols;
    let cell_h = img.height() / rows;
    if cell_w == 0 || cell_h == 0 {
        return Err(CliError::new(2, "grid is larger than image"));
    }
    if img.width() % cols != 0 || img.height() % rows != 0 {
        ctx.event(
            "warning",
            json!({
                "message": format!(
                    "{}x{} image does not divide evenly by a {cols}x{rows} grid; {}px column / {}px row remainder is dropped",
                    img.width(), img.height(), img.width() % cols, img.height() % rows
                )
            }),
        );
    }
    fs::create_dir_all(&args.out_dir)?;
    // `--overwrite` is a clean replacement: clear stale frames from a prior slice
    // so a later sheet-pack can't pick up orphaned frames.
    clear_frame_pngs(&args.out_dir)?;
    let mut index = 0;
    for r in 0..rows {
        for c in 0..cols {
            let frame = imageops::crop_imm(&img, c * cell_w, r * cell_h, cell_w, cell_h).to_image();
            let out = args.out_dir.join(format!("frame_{index:03}.png"));
            save_rgba_atomic(&out, &frame, args.overwrite)?;
            ctx.event("artifact", json!({"path": out, "kind": "image"}));
            index += 1;
        }
    }
    Ok(())
}

fn sprite_sheet_pack(ctx: &Ctx, args: SheetPackArgs) -> Result<()> {
    ensure_output(&args.out, args.overwrite)?;
    // Guard the sidecar with the same existence check as the image so a metadata
    // path is never clobbered without --overwrite, and the check happens before
    // any work rather than after the sheet is already written.
    if let Some(meta) = &args.metadata_out {
        check_output_available(meta, args.overwrite)?;
    }
    validate_positive_u32("--cols", args.cols)?;
    let mut files: Vec<PathBuf> = fs::read_dir(&args.input_dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| kind_from_ext(p) == "image")
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(CliError::new(3, "no image frames found"));
    }
    let first = image::open(&files[0])
        .map_err(|e| CliError::new(3, format!("decode image: {e}")))?
        .to_rgba8();
    let frame_w = first.width();
    let frame_h = first.height();
    let cols = args.cols;
    let rows = (files.len() as u32).div_ceil(cols);
    let (sheet_w, sheet_h) = checked_canvas_dims(cols, rows, frame_w, frame_h)?;
    let mut sheet = RgbaImage::from_pixel(sheet_w, sheet_h, Rgba([0, 0, 0, 0]));
    let mut frames = Vec::new();
    for (i, file) in files.iter().enumerate() {
        let img = image::open(file)
            .map_err(|e| CliError::new(3, format!("{}: {e}", file.display())))?
            .to_rgba8();
        if img.width() != frame_w || img.height() != frame_h {
            return Err(CliError::new(
                3,
                format!("frame size mismatch: {}", file.display()),
            ));
        }
        let col = i as u32 % cols;
        let row = i as u32 / cols;
        let x = col * frame_w;
        let y = row * frame_h;
        imageops::overlay(&mut sheet, &img, x.into(), y.into());
        frames.push(json!({
            "file": file.file_name().and_then(OsStr::to_str).unwrap_or(""),
            "x": x,
            "y": y,
            "w": frame_w,
            "h": frame_h
        }));
    }
    save_rgba_atomic(&args.out, &sheet, args.overwrite)?;
    if let Some(meta) = args.metadata_out {
        write_json_atomic(
            &meta,
            &json!({"image": args.out, "frame_width": frame_w, "frame_height": frame_h, "frames": frames}),
            args.overwrite,
        )?;
    }
    ctx.event("artifact", json!({"path": args.out, "kind": "image"}));
    Ok(())
}

fn video_slice(ctx: &Ctx, args: VideoSliceArgs) -> Result<()> {
    validate_nonnegative_f32("--start", args.start)?;
    validate_nonnegative_f32("--end", args.end)?;
    if args.end <= args.start {
        return Err(CliError::new(2, "--end must be greater than --start"));
    }
    if args.frames == 0 || args.frames > 64 {
        return Err(CliError::new(2, "--frames must be between 1 and 64"));
    }
    if !args.input.is_file() {
        return Err(CliError::new(
            3,
            format!("input not found: {}", args.input.display()),
        ));
    }
    if which::which("ffmpeg").is_err() {
        return Err(CliError::new(5, "ffmpeg not found in PATH"));
    }
    // Resolve --key BEFORE shelling out to ffmpeg. An invalid color used to be
    // parsed only after frames were written, so a typo left orphaned PNGs behind
    // and failed late. "auto" is resolved from the produced frames further down.
    let key_mode = match args.key.as_deref() {
        None => None,
        Some("auto") => Some(KeyMode::Auto),
        Some(hex) => Some(KeyMode::Fixed(parse_hex_color(hex)?)),
    };
    if args.out_dir.exists() && !args.overwrite {
        return Err(CliError::new(
            4,
            format!("output directory exists: {}", args.out_dir.display()),
        ));
    }
    fs::create_dir_all(&args.out_dir)?;
    // `--overwrite` means a clean replacement: drop any frame_*.png from a prior
    // run so stale frames can't be reported as new artifacts or packed downstream.
    clear_frame_pngs(&args.out_dir)?;
    let frames = args.frames;
    let duration = args.end - args.start;
    let fps = frames as f32 / duration;
    let pattern = args.out_dir.join("frame_%03d.png");
    // Note: deliberately no `-t {duration}`. The `fps` filter emits a boundary
    // frame at t=duration which `-t` would truncate, causing an off-by-one
    // shortfall (e.g. 64 requested -> 63 produced). `-frames:v` alone caps the
    // count exactly while letting that final frame through.
    let status = Command::new("ffmpeg")
        .arg("-y")
        .arg("-nostdin")
        .arg("-loglevel")
        .arg("error")
        .arg("-ss")
        .arg(args.start.to_string())
        .arg("-i")
        .arg(&args.input)
        .arg("-vf")
        .arg(format!("fps={fps:.4}"))
        .arg("-frames:v")
        .arg(frames.to_string())
        .arg(&pattern)
        .status()
        .map_err(|e| CliError::new(6, format!("run ffmpeg: {e}")))?;
    if !status.success() {
        return Err(CliError::new(6, "ffmpeg failed"));
    }
    // ffmpeg exits 0 even when seeking past EOF or given a still image, producing
    // zero frames. Treat "no frames" as a hard error, and a partial count (e.g.
    // --end beyond the clip duration) as a warning rather than a silent success.
    let produced = sorted_pngs(&args.out_dir)?.len() as u32;
    if produced == 0 {
        return Err(CliError::new(
            6,
            "no frames produced: --start may be past the end of the video, or the input has no decodable video stream",
        ));
    }
    if produced < frames {
        ctx.event(
            "warning",
            json!({
                "message": format!(
                    "requested {frames} frames but only {produced} were produced; the [{}, {}]s range likely exceeds the video duration",
                    args.start, args.end
                ),
                "requested": frames,
                "produced": produced
            }),
        );
    }
    if let Some(mode) = key_mode {
        let files = sorted_pngs(&args.out_dir)?;
        // "auto" detects the background color from the corners of the first frame;
        // a fixed hex keys that exact color. Either way every frame is keyed out.
        let key = match mode {
            KeyMode::Fixed(k) => k,
            KeyMode::Auto => detect_key_color(&files)?,
        };
        for file in &files {
            let mut rgba = image::open(file)
                .map_err(|e| CliError::new(3, format!("{}: {e}", file.display())))?
                .to_rgba8();
            for pixel in rgba.pixels_mut() {
                if color_distance(pixel.0, key) <= 42.0 {
                    pixel.0[3] = 0;
                }
            }
            save_rgba_atomic(file, &rgba, true)?;
        }
    }
    for file in sorted_pngs(&args.out_dir)? {
        ctx.event("artifact", json!({"path": file, "kind": "image"}));
    }
    Ok(())
}

async fn audio_bgm(ctx: &Ctx, args: AudioBgmArgs) -> Result<()> {
    check_output_available(&args.out, args.overwrite)?;
    validate_sample_rate(args.sample_rate)?;
    validate_positive_u32("--bitrate", args.bitrate)?;
    let prompt = read_prompt(args.prompt.as_deref(), args.prompt_text.as_deref())?;
    // Only treat lyrics as absent when the user supplied neither source. If they
    // did supply one, propagate read errors (missing file, both flags) instead of
    // swallowing them with .ok() — otherwise a typo'd path silently yields empty
    // lyrics, or misreports as "vocals require --lyrics".
    let lyrics = if args.lyrics.is_some() || args.lyrics_text.is_some() {
        Some(read_prompt(
            args.lyrics.as_deref(),
            args.lyrics_text.as_deref(),
        )?)
    } else {
        None
    };
    if !args.instrumental && lyrics.is_none() && !args.lyrics_optimizer {
        return Err(CliError::new(
            2,
            "vocals require --lyrics/--lyrics-text or --lyrics-optimizer",
        ));
    }
    warn_format_extension(ctx, &args.out, &args.format);
    if args.dry_run {
        let mut event = json!({"provider": "minimax-music", "model": args.model, "dry_run": true});
        if ctx.include_prompts {
            event["prompt"] = Value::String(prompt.clone());
        }
        ctx.event("provider_request", event);
        return Ok(());
    }
    let key =
        env::var("MINIMAX_API_KEY").map_err(|_| CliError::new(5, "MINIMAX_API_KEY is not set"))?;
    let mut request_event = json!({"provider": "minimax-music", "model": args.model});
    if ctx.include_prompts {
        request_event["prompt"] = Value::String(prompt.clone());
    }
    ctx.event("provider_request", request_event);
    let req = json!({
        "model": args.model,
        "prompt": prompt,
        "lyrics": lyrics.unwrap_or_default(),
        "lyrics_optimizer": args.lyrics_optimizer,
        "is_instrumental": args.instrumental,
        "output_format": "hex",
        "audio_setting": {
            "sample_rate": args.sample_rate,
            "bitrate": args.bitrate,
            "format": args.format,
        }
    });
    let client = reqwest::Client::new();
    let resp = client
        .post("https://api.minimax.io/v1/music_generation")
        .bearer_auth(key)
        .json(&req)
        .send()
        .await
        .map_err(|e| CliError::new(6, format!("MiniMax request failed: {e}")))?;
    let status = resp.status();
    let body: Value = resp
        .json()
        .await
        .map_err(|e| CliError::new(6, format!("MiniMax response was not JSON: {e}")))?;
    if !status.is_success() {
        return Err(CliError::new(6, format!("MiniMax HTTP {status}: {body}")));
    }
    let status_code = body
        .pointer("/base_resp/status_code")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if status_code != 0 {
        return Err(CliError::new(6, format!("MiniMax error: {body}")));
    }
    let audio_hex = body
        .pointer("/data/audio")
        .and_then(Value::as_str)
        .ok_or_else(|| CliError::new(6, "MiniMax response missing data.audio"))?;
    let bytes =
        hex::decode(audio_hex).map_err(|e| CliError::new(6, format!("decode hex audio: {e}")))?;
    write_atomic(&args.out, &bytes, args.overwrite)?;
    if let Some(path) = args.metadata_out {
        // Don't duplicate the entire (hex-encoded) audio payload into the sidecar.
        let mut meta = body.clone();
        if let Some(audio) = meta.pointer_mut("/data/audio") {
            *audio = Value::String(format!("<{} bytes omitted>", bytes.len()));
        }
        let summary = json!({
            "provider": "minimax-music",
            "model": args.model,
            "format": args.format,
            "sample_rate": args.sample_rate,
            "bitrate": args.bitrate,
            "audio_bytes": bytes.len(),
            "response": meta,
        });
        write_json_atomic(&path, &summary, true)?;
    }
    ctx.event(
        "artifact",
        json!({"path": args.out, "kind": "audio", "bytes": bytes.len()}),
    );
    Ok(())
}

fn audio_sfx(ctx: &Ctx, args: AudioSfxArgs) -> Result<()> {
    validate_positive_u32("--duration-ms", args.duration_ms)?;
    validate_sample_rate(args.sample_rate)?;
    validate_positive_u32("--variations", args.variations)?;
    if let Some(pitch) = args.pitch {
        validate_positive_f32("--pitch", pitch)?;
    }
    if args.variations <= 1 {
        let out = args
            .out
            .ok_or_else(|| CliError::new(2, "--out is required when --variations is 1"))?;
        ensure_output(&out, args.overwrite)?;
        render_sfx_file(
            &out,
            args.preset,
            args.duration_ms,
            args.pitch,
            args.seed,
            args.sample_rate,
            args.overwrite,
        )?;
        ctx.event("artifact", json!({"path": out, "kind": "audio"}));
    } else {
        let dir = args
            .out_dir
            .ok_or_else(|| CliError::new(2, "--out-dir is required when --variations > 1"))?;
        fs::create_dir_all(&dir)?;
        for i in 0..args.variations {
            let out = dir.join(format!("{}_{:02}.wav", preset_name(args.preset), i + 1));
            ensure_output(&out, args.overwrite)?;
            render_sfx_file(
                &out,
                args.preset,
                args.duration_ms,
                args.pitch,
                args.seed.wrapping_add(i as u64),
                args.sample_rate,
                args.overwrite,
            )?;
            ctx.event("artifact", json!({"path": out, "kind": "audio"}));
        }
    }
    Ok(())
}

fn audio_trim(ctx: &Ctx, args: AudioTrimArgs) -> Result<()> {
    validate_nonnegative_f32("--start", args.start)?;
    validate_nonnegative_f32("--end", args.end)?;
    if args.end <= args.start {
        return Err(CliError::new(2, "--end must be greater than --start"));
    }
    ensure_output(&args.out, args.overwrite)?;
    require_wav_extension(&args.input)?;
    let mut reader = hound::WavReader::open(&args.input)
        .map_err(|e| CliError::new(3, format!("open wav: {e}")))?;
    let spec = reader.spec();
    let channels = spec.channels as usize;
    let start = (args.start * spec.sample_rate as f32) as usize * channels;
    let end = (args.end * spec.sample_rate as f32) as usize * channels;
    let total_samples = reader.len() as usize;
    if start >= total_samples {
        let duration =
            total_samples as f32 / (spec.sample_rate.max(1) as f32 * channels.max(1) as f32);
        return Err(CliError::new(
            2,
            format!(
                "--start ({:.3}s) is at or beyond input duration ({:.3}s)",
                args.start, duration
            ),
        ));
    }
    let tmp = temp_path_for(&args.out);
    let result: Result<()> = (|| {
        let mut writer = hound::WavWriter::create(&tmp, spec)
            .map_err(|e| CliError::new(1, format!("create wav: {e}")))?;
        // Read with a sample type that matches the file's format/bit depth so
        // 8/16/24/32-bit PCM and 32-bit float all round-trip without loss.
        match spec.sample_format {
            hound::SampleFormat::Int => {
                let samples: Vec<i32> = reader
                    .samples::<i32>()
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(|e| CliError::new(3, format!("read wav samples: {e}")))?;
                let (start, end) = (start.min(samples.len()), end.min(samples.len()));
                for s in &samples[start..end] {
                    writer
                        .write_sample(*s)
                        .map_err(|e| CliError::new(1, e.to_string()))?;
                }
            }
            hound::SampleFormat::Float => {
                let samples: Vec<f32> = reader
                    .samples::<f32>()
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(|e| CliError::new(3, format!("read wav samples: {e}")))?;
                let (start, end) = (start.min(samples.len()), end.min(samples.len()));
                for s in &samples[start..end] {
                    writer
                        .write_sample(*s)
                        .map_err(|e| CliError::new(1, e.to_string()))?;
                }
            }
        }
        writer
            .finalize()
            .map_err(|e| CliError::new(1, e.to_string()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result?;
    fs::rename(&tmp, &args.out).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        CliError::from(e)
    })?;
    ctx.event("artifact", json!({"path": args.out, "kind": "audio"}));
    Ok(())
}

fn audio_waveform(ctx: &Ctx, args: AudioWaveformArgs) -> Result<()> {
    validate_positive_u32("--width", args.width)?;
    validate_positive_u32("--height", args.height)?;
    ensure_output(&args.out, args.overwrite)?;
    require_wav_extension(&args.input)?;
    let samples = read_wav_normalized(&args.input)?;
    let mut img = RgbaImage::from_pixel(args.width, args.height, Rgba([12, 14, 18, 255]));
    if !samples.is_empty() {
        for x in 0..args.width {
            let a = (x as usize * samples.len()) / args.width as usize;
            let b = (((x + 1) as usize * samples.len()) / args.width as usize)
                .max(a + 1)
                .min(samples.len());
            let peak = samples[a..b].iter().map(|s| s.abs()).fold(0.0, f32::max);
            let half = (peak * args.height as f32 * 0.46) as i32;
            let mid = args.height as i32 / 2;
            for y in (mid - half).max(0)..=(mid + half).min(args.height as i32 - 1) {
                img.put_pixel(x, y as u32, Rgba([90, 220, 150, 255]));
            }
        }
    }
    save_rgba_atomic(&args.out, &img, args.overwrite)?;
    ctx.event("artifact", json!({"path": args.out, "kind": "image"}));
    Ok(())
}

fn contact_sheet(ctx: &Ctx, args: ContactSheetArgs) -> Result<()> {
    validate_positive_u32("--cols", args.cols)?;
    if args.cell <= 12 {
        return Err(CliError::new(2, "--cell must be greater than 12"));
    }
    ensure_output(&args.out, args.overwrite)?;
    let mut files = Vec::new();
    for pattern in &args.inputs {
        for entry in glob::glob(pattern).map_err(|e| CliError::new(2, e.to_string()))? {
            files.push(entry.map_err(|e| CliError::new(3, e.to_string()))?);
        }
    }
    files.sort();
    if files.is_empty() {
        return Err(CliError::new(3, "no input images matched"));
    }
    let cols = args.cols;
    let rows = (files.len() as u32).div_ceil(cols);
    let (sheet_w, sheet_h) = checked_canvas_dims(cols, rows, args.cell, args.cell)?;
    let mut sheet = RgbaImage::from_pixel(sheet_w, sheet_h, Rgba([24, 26, 30, 255]));
    for (i, file) in files.iter().enumerate() {
        let img =
            image::open(file).map_err(|e| CliError::new(3, format!("{}: {e}", file.display())))?;
        let thumb = img
            .resize(
                args.cell - 12,
                args.cell - 12,
                imageops::FilterType::Lanczos3,
            )
            .to_rgba8();
        let col = i as u32 % cols;
        let row = i as u32 / cols;
        let x = col * args.cell + (args.cell - thumb.width()) / 2;
        let y = row * args.cell + (args.cell - thumb.height()) / 2;
        imageops::overlay(&mut sheet, &thumb, x.into(), y.into());
    }
    save_rgba_atomic(&args.out, &sheet, args.overwrite)?;
    ctx.event("artifact", json!({"path": args.out, "kind": "image"}));
    Ok(())
}

#[derive(Serialize)]
struct ManifestEntry {
    path: String,
    kind: String,
    bytes: u64,
    sha256: String,
}

fn manifest(ctx: &Ctx, args: ManifestArgs) -> Result<()> {
    ensure_output(&args.out, args.overwrite)?;
    let mut entries = Vec::new();
    for entry in walkdir::WalkDir::new(&args.input) {
        let entry = entry.map_err(|e| CliError::new(3, e.to_string()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let bytes = fs::read(path)?;
        let rel = path.strip_prefix(&args.input).unwrap_or(path);
        // When --in points at a single file, strip_prefix yields an empty path;
        // fall back to the file name so the entry is still identifiable.
        let rel = if rel.as_os_str().is_empty() {
            Path::new(path.file_name().unwrap_or(OsStr::new("file")))
        } else {
            rel
        };
        entries.push(ManifestEntry {
            path: rel.to_string_lossy().replace('\\', "/"),
            kind: kind_from_ext(path).to_string(),
            bytes: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(&bytes)),
        });
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    write_json_atomic(
        &args.out,
        &json!({"version": 1, "assets": entries}),
        args.overwrite,
    )?;
    ctx.event("artifact", json!({"path": args.out, "kind": "manifest"}));
    Ok(())
}

fn doctor(ctx: &Ctx) -> Result<()> {
    let codex_bin = env::var("CODEX_BIN").unwrap_or_else(|_| "codex".into());
    let codex = resolve_executable(&codex_bin);
    let codex_version = codex
        .as_ref()
        .and_then(|path| command_first_stdout_line(path, ["--version"]));
    let codex_exec = codex
        .as_ref()
        .map(|path| command_output_contains(path, ["exec", "--help"], "codex exec"))
        .unwrap_or(false);
    let ffmpeg = which::which("ffmpeg").ok();
    let minimax = env::var("MINIMAX_API_KEY").is_ok();
    let sandbox = env::var("CODEX_SANDBOX").unwrap_or_else(|_| "workspace-write".into());
    let sandbox_allowed = matches!(
        sandbox.as_str(),
        "workspace-write" | "read-only" | "danger-full-access"
    );
    let temp_dir_writable = temp_dir_is_writable();
    if ctx.json {
        ctx.event(
            "doctor",
            json!({
                "codex": codex.as_ref().map(|p| p.display().to_string()),
                "codex_version": codex_version,
                "codex_exec": codex_exec,
                "codex_image_probe": "not_run",
                "codex_sandbox": sandbox,
                "codex_sandbox_allowed": sandbox_allowed,
                "ffmpeg": ffmpeg.as_ref().map(|p| p.display().to_string()),
                "minimax_api_key": minimax,
                "temp_dir_writable": temp_dir_writable
            }),
        );
    } else if !ctx.quiet {
        println!(
            "codex: {}",
            codex
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "missing".into())
        );
        println!(
            "codex version: {}",
            codex_version.unwrap_or_else(|| "unknown".into())
        );
        println!(
            "codex exec: {}",
            if codex_exec {
                "available"
            } else {
                "unavailable"
            }
        );
        println!("codex image probe: not run");
        println!(
            "CODEX_SANDBOX: {}",
            if sandbox_allowed {
                sandbox
            } else {
                format!("{sandbox} (invalid)")
            }
        );
        println!(
            "ffmpeg: {}",
            ffmpeg
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "missing".into())
        );
        println!(
            "MINIMAX_API_KEY: {}",
            if minimax { "set" } else { "missing" }
        );
        println!(
            "temp dir writable: {}",
            if temp_dir_writable { "yes" } else { "no" }
        );
    }
    Ok(())
}

const USER_AGENT: &str = concat!("game-asset/", env!("CARGO_PKG_VERSION"));

async fn upgrade(ctx: &Ctx, args: UpgradeArgs) -> Result<()> {
    let target = current_target()?;
    let current = env!("CARGO_PKG_VERSION");
    let client = reqwest::Client::new();

    // Resolve the release to install: a specific tag, or the latest.
    let release_url = match &args.tag {
        Some(tag) => format!(
            "https://api.github.com/repos/{}/releases/tags/{}",
            args.repo, tag
        ),
        None => format!("https://api.github.com/repos/{}/releases/latest", args.repo),
    };
    let mut req = client
        .get(&release_url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json");
    if let Some(token) = github_token() {
        req = req.bearer_auth(token);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| CliError::new(6, format!("GitHub request failed: {e}")))?;
    let status = resp.status();
    let body: Value = resp
        .json()
        .await
        .map_err(|e| CliError::new(6, format!("GitHub response was not JSON: {e}")))?;
    if !status.is_success() {
        let detail = body
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        let hint = if github_token().is_none() {
            " (set GITHUB_TOKEN for private repositories or to raise rate limits)"
        } else {
            ""
        };
        return Err(CliError::new(
            6,
            format!("GitHub HTTP {status} for {}: {detail}{hint}", args.repo),
        ));
    }
    let tag = body
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or_else(|| CliError::new(6, "release is missing tag_name"))?
        .to_string();
    let latest = tag.trim_start_matches('v').to_string();
    let newer = version_is_newer(&latest, current);

    // --check never installs; it only reports the comparison.
    if args.check {
        ctx.event(
            "upgrade",
            json!({
                "current": current,
                "latest": latest,
                "tag": tag,
                "target": target,
                "update_available": newer,
            }),
        );
        if !ctx.json && !ctx.quiet {
            if newer {
                println!("update available: {current} -> {latest}");
            } else {
                println!("up to date ({current})");
            }
        }
        return Ok(());
    }

    // No-op when already current unless the caller forces a reinstall.
    if !newer && !args.force {
        ctx.event(
            "upgrade",
            json!({
                "current": current,
                "latest": latest,
                "tag": tag,
                "target": target,
                "updated": false,
                "reason": "already up to date",
            }),
        );
        if !ctx.json && !ctx.quiet {
            println!("already up to date ({current}); use --force to reinstall");
        }
        return Ok(());
    }

    // Pick the asset built for this platform by matching the target triple.
    let asset = body
        .get("assets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|a| {
            a.get("name")
                .and_then(Value::as_str)
                .map(|n| n.contains(&target))
                .unwrap_or(false)
        })
        .ok_or_else(|| {
            CliError::new(6, format!("release {tag} has no asset for target {target}"))
        })?;
    let asset_name = asset
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("asset")
        .to_string();
    let asset_url = asset
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| CliError::new(6, "asset is missing its API url"))?
        .to_string();

    let dest = env::current_exe()
        .map_err(|e| CliError::new(1, format!("cannot locate current executable: {e}")))?;

    if args.dry_run {
        ctx.event(
            "upgrade",
            json!({
                "current": current,
                "latest": latest,
                "tag": tag,
                "target": target,
                "asset": asset_name,
                "dest": dest.display().to_string(),
                "updated": false,
                "dry_run": true,
            }),
        );
        if !ctx.json && !ctx.quiet {
            println!(
                "would install {asset_name} ({current} -> {latest}) to {}",
                dest.display()
            );
        }
        return Ok(());
    }

    ctx.event(
        "provider_request",
        json!({"provider": "github-release", "asset": asset_name, "tag": tag}),
    );

    // The asset API url plus an octet-stream Accept header serves the raw bytes
    // for both public and private repositories. reqwest drops the Authorization
    // header on the cross-host redirect to the CDN, so the signed URL is honored.
    let mut dl = client
        .get(&asset_url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header(reqwest::header::ACCEPT, "application/octet-stream");
    if let Some(token) = github_token() {
        dl = dl.bearer_auth(token);
    }
    let dl_resp = dl
        .send()
        .await
        .map_err(|e| CliError::new(6, format!("asset download failed: {e}")))?;
    let dl_status = dl_resp.status();
    if !dl_status.is_success() {
        return Err(CliError::new(6, format!("asset download HTTP {dl_status}")));
    }
    let bytes = dl_resp
        .bytes()
        .await
        .map_err(|e| CliError::new(6, format!("reading asset body: {e}")))?;

    // Unpack into a scratch dir and locate the new binary inside it.
    let work = TempDir::new().map_err(CliError::from)?;
    let archive = work.path().join(&asset_name);
    fs::write(&archive, &bytes)?;
    extract_archive(&archive, work.path())?;
    let new_bin = find_binary(work.path())?;

    replace_executable(&new_bin, &dest)?;

    ctx.event(
        "artifact",
        json!({
            "path": dest.display().to_string(),
            "kind": "binary",
            "version": latest,
            "tag": tag,
            "bytes": bytes.len(),
        }),
    );
    if !ctx.json && !ctx.quiet {
        println!("upgraded {current} -> {latest} ({})", dest.display());
    }
    Ok(())
}

fn github_token() -> Option<String> {
    env::var("GITHUB_TOKEN")
        .ok()
        .or_else(|| env::var("GH_TOKEN").ok())
        .filter(|t| !t.is_empty())
}

fn current_target() -> Result<String> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let triple = match (os, arch) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        _ => {
            return Err(CliError::new(
                2,
                format!("no prebuilt binary is published for {os}/{arch}"),
            ))
        }
    };
    Ok(triple.to_string())
}

// Compare dotted numeric versions (e.g. "0.2.0" vs "0.1.0") without pulling in a
// semver dependency. Non-numeric pre-release segments collapse to 0, which is
// sufficient for the project's plain vMAJOR.MINOR.PATCH tags.
fn version_is_newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> {
        v.split(['.', '-', '+'])
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let a = parse(candidate);
    let b = parse(current);
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}

fn extract_archive(archive: &Path, dest: &Path) -> Result<()> {
    // `tar -xf` auto-detects gzip on every platform, and bsdtar (macOS, modern
    // Windows) also unpacks the .zip asset, so one invocation covers all targets.
    let status = Command::new("tar")
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(dest)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| CliError::new(1, format!("failed to run tar: {e}")))?;
    if !status.success() {
        return Err(CliError::new(
            1,
            format!("tar failed to extract {}", archive.display()),
        ));
    }
    Ok(())
}

fn find_binary(root: &Path) -> Result<PathBuf> {
    let want = if cfg!(windows) {
        "game-asset.exe"
    } else {
        "game-asset"
    };
    for entry in walkdir::WalkDir::new(root).into_iter().flatten() {
        if entry.file_type().is_file() && entry.file_name() == OsStr::new(want) {
            return Ok(entry.path().to_path_buf());
        }
    }
    Err(CliError::new(
        1,
        format!("downloaded archive did not contain {want}"),
    ))
}

#[cfg(unix)]
fn replace_executable(new_bin: &Path, dest: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let dest = dest.canonicalize().unwrap_or_else(|_| dest.to_path_buf());
    let dir = dest.parent().unwrap_or_else(|| Path::new("."));
    // Stage beside the destination so the final swap is a same-filesystem rename
    // (atomic), and replacing a running binary keeps the old inode alive.
    let staged = dir.join(format!(".game-asset.upgrade.{}", std::process::id()));
    fs::copy(new_bin, &staged).map_err(|e| {
        let _ = fs::remove_file(&staged);
        CliError::new(
            1,
            format!(
                "cannot write to {} ({e}); rerun with adequate permissions",
                dir.display()
            ),
        )
    })?;
    if let Err(e) = fs::set_permissions(&staged, fs::Permissions::from_mode(0o755)) {
        let _ = fs::remove_file(&staged);
        return Err(CliError::from(e));
    }
    fs::rename(&staged, &dest).map_err(|e| {
        let _ = fs::remove_file(&staged);
        CliError::new(
            1,
            format!("cannot install new executable to {}: {e}", dest.display()),
        )
    })?;
    Ok(())
}

#[cfg(windows)]
fn replace_executable(new_bin: &Path, dest: &Path) -> Result<()> {
    let dest = dest.canonicalize().unwrap_or_else(|_| dest.to_path_buf());
    let dir = dest.parent().unwrap_or_else(|| Path::new("."));
    let staged = dir.join(format!(".game-asset.upgrade.{}.exe", std::process::id()));
    fs::copy(new_bin, &staged).map_err(|e| {
        let _ = fs::remove_file(&staged);
        CliError::new(
            1,
            format!(
                "cannot write to {} ({e}); rerun with adequate permissions",
                dir.display()
            ),
        )
    })?;
    // Windows cannot overwrite a running executable, so move it aside first and
    // restore it if the swap fails. The backup is left in place because the old
    // image may stay locked until this process exits.
    let backup = dir.join(format!(".game-asset.old.{}.exe", std::process::id()));
    let _ = fs::remove_file(&backup);
    fs::rename(&dest, &backup).map_err(|e| {
        let _ = fs::remove_file(&staged);
        CliError::new(1, format!("cannot move current executable aside: {e}"))
    })?;
    if let Err(e) = fs::rename(&staged, &dest) {
        let _ = fs::rename(&backup, &dest);
        let _ = fs::remove_file(&staged);
        return Err(CliError::new(
            1,
            format!("cannot install new executable: {e}"),
        ));
    }
    let _ = fs::remove_file(&backup);
    Ok(())
}

fn resolve_executable(candidate: &str) -> Option<PathBuf> {
    let path = Path::new(candidate);
    if path.components().count() > 1 {
        return path.is_file().then(|| path.to_path_buf());
    }
    which::which(candidate).ok()
}

fn command_output_contains<const N: usize>(path: &Path, args: [&str; N], needle: &str) -> bool {
    let output = match Command::new(path).args(args).output() {
        Ok(output) => output,
        Err(_) => return false,
    };
    if !output.status.success() {
        return false;
    }
    String::from_utf8_lossy(&output.stdout).contains(needle)
        || String::from_utf8_lossy(&output.stderr).contains(needle)
}

fn command_first_stdout_line<const N: usize>(path: &Path, args: [&str; N]) -> Option<String> {
    let output = Command::new(path).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(str::to_string)
}

fn temp_dir_is_writable() -> bool {
    let dir = match TempDir::new() {
        Ok(dir) => dir,
        Err(_) => return false,
    };
    let path = dir.path().join("game-asset-doctor.tmp");
    fs::write(&path, b"ok")
        .and_then(|_| fs::remove_file(path))
        .is_ok()
}

#[allow(clippy::too_many_arguments)]
async fn run_codex_image(
    ctx: &Ctx,
    instruction: &str,
    refs: &[PathBuf],
    out: &Path,
    overwrite: bool,
    metadata_out: Option<&Path>,
    model: Option<&str>,
    requested_size: Option<(u32, u32)>,
    green_key: Option<[u8; 3]>,
    timeout_seconds: u64,
    dry_run: bool,
) -> Result<()> {
    let mut canonical_refs = Vec::new();
    for r in refs {
        if !r.is_file() {
            return Err(CliError::new(
                3,
                format!("reference image not found: {}", r.display()),
            ));
        }
        canonical_refs.push(
            fs::canonicalize(r).map_err(|e| CliError::new(3, format!("{}: {e}", r.display())))?,
        );
    }
    // We pick the result up from `$CODEX_HOME/generated_images` (codex always
    // writes there), so the save path below is not how we find the file — but an
    // explicit "generate and save to a file" directive is load-bearing for
    // reliably triggering codex's image tool. Without it, codex often treats the
    // framed brief as a broader agentic task and never generates at all.
    let rel_out = "asset.png";
    let final_instruction = format!(
        "{instruction}\n\n\
Generate the image for THIS request now with your image generation tool, then save it to `./{rel_out}` in the current working directory.\n\
- If image generation fails, exit with a non-zero status."
    );
    // Logged here (before the dry-run return) so `--verbose --dry-run` previews
    // the exact instruction without ever invoking codex.
    ctx.vlog(format!(
        "full provider instruction follows:\n{final_instruction}"
    ));
    if dry_run {
        let mut event = json!({
            "provider": "codex-image",
            "dry_run": true,
            "timeout_seconds": timeout_seconds,
            "refs": canonical_refs,
            "requested_size": requested_size.map(|(w, h)| json!({"width": w, "height": h}))
        });
        if ctx.include_prompts {
            event["prompt"] = Value::String(instruction.to_string());
        }
        ctx.event("provider_request", event);
        return Ok(());
    }

    let codex_bin = env::var("CODEX_BIN").unwrap_or_else(|_| "codex".into());
    let codex_path = resolve_executable(&codex_bin)
        .ok_or_else(|| CliError::new(5, format!("{codex_bin} not found or not executable")))?;
    let sandbox = codex_sandbox()?;
    let tmpdir = TempDir::new().map_err(|e| CliError::new(1, e.to_string()))?;
    if ctx.verbose {
        ctx.vlog(format!("codex binary: {}", codex_path.display()));
        ctx.vlog(format!("sandbox: {sandbox}"));
        ctx.vlog(format!("working dir: {}", tmpdir.path().display()));
        ctx.vlog(format!("model: {}", model.unwrap_or("(default)")));
        ctx.vlog(format!(
            "reference images: {}",
            if canonical_refs.is_empty() {
                "(none)".to_string()
            } else {
                canonical_refs
                    .iter()
                    .map(|r| r.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        ));
        ctx.vlog(format!("timeout: {timeout_seconds}s"));
        // The exact codex invocation. The instruction arg is shown as a
        // placeholder because its full text is already printed above.
        let mut argv = vec![
            shell_quote(&codex_path.to_string_lossy()),
            "exec".into(),
            "--skip-git-repo-check".into(),
            "--sandbox".into(),
            shell_quote(&sandbox),
            "-C".into(),
            shell_quote(&tmpdir.path().to_string_lossy()),
        ];
        if let Some(m) = model {
            argv.push("--model".into());
            argv.push(shell_quote(m));
        }
        argv.push("'<instruction shown above>'".into());
        for r in &canonical_refs {
            argv.push("--image".into());
            argv.push(shell_quote(&r.to_string_lossy()));
        }
        ctx.vlog(format!("codex command: {}", argv.join(" ")));
    }
    let mut cmd = TokioCommand::new(codex_path);
    // NOTE: do not pass --ephemeral. Under --ephemeral codex does not persist the
    // session rollout, so the agent's image_gen call can fail to materialize and
    // the run hangs until timeout. A normal (persisted) session generates the
    // image reliably.
    cmd.arg("exec")
        .arg("--skip-git-repo-check")
        .arg("--sandbox")
        .arg(&sandbox)
        .arg("-C")
        .arg(tmpdir.path());
    cmd.kill_on_drop(true);
    if let Some(model) = model {
        cmd.arg("--model").arg(model);
    }
    cmd.arg(final_instruction);
    for r in &canonical_refs {
        cmd.arg("--image").arg(r);
    }
    let mut request_event = json!({"provider": "codex-image", "timeout_seconds": timeout_seconds});
    if ctx.include_prompts {
        request_event["prompt"] = Value::String(instruction.to_string());
    }
    ctx.event("provider_request", request_event);

    // Drive codex by polling `generated_images` for the new file instead of
    // blocking on full process exit. After writing the image codex may run extra
    // confirmation steps that extend its lifetime past the timeout; waiting on
    // exit would kill and discard a perfectly good, already-written image.
    // Instead: as soon as a fresh PNG is on disk with a stable size, stop codex
    // and use what it produced.
    //
    // Both codex streams are piped and drained continuously by their own tasks.
    // Draining is what prevents a deadlock: if a pipe filled without anyone
    // reading, codex would block before ever writing the image. Under --verbose we
    // echo each line straight to OUR stderr as it arrives, so the agent log streams
    // live regardless of whether stdout is a TTY (codex hides its pretty log when
    // stdout is not a terminal) and survives the timeout path. Echoing to stderr
    // (never stdout) keeps the --json contract on stdout uncorrupted. stderr is
    // additionally accumulated so its tail is available for error reporting.
    // Capture the launch instant and codex's image dir before spawning. codex
    // writes the PNG into `$CODEX_HOME/generated_images`; that dir accumulates
    // images across sessions, so we only ever trust files modified at/after
    // `run_start` — those belong to this run.
    let run_start = SystemTime::now();
    let images_dir = codex_generated_images_dir().ok_or_else(|| {
        CliError::new(
            1,
            "cannot locate codex generated_images dir (set CODEX_HOME or HOME)",
        )
    })?;
    ctx.vlog(format!(
        "watching codex image dir: {}",
        images_dir.display()
    ));
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| CliError::new(6, format!("run codex: {e}")))?;
    let verbose = ctx.verbose;
    if let Some(out) = child.stdout.take() {
        tokio::spawn(async move {
            use tokio::io::{AsyncBufReadExt, BufReader};
            let mut lines = BufReader::new(out).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if verbose {
                    eprintln!("{line}");
                }
            }
        });
    }
    let stderr_tail = std::sync::Arc::new(Mutex::new(String::new()));
    if let Some(err) = child.stderr.take() {
        let sink = stderr_tail.clone();
        tokio::spawn(async move {
            use tokio::io::{AsyncBufReadExt, BufReader};
            let mut lines = BufReader::new(err).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if verbose {
                    eprintln!("{line}");
                }
                let mut buf = sink.lock().unwrap();
                buf.push_str(&line);
                buf.push('\n');
            }
        });
    }
    // The freshly-generated PNG is the newest file in `generated_images` whose
    // mtime is at/after launch.
    let find = || newest_generated_png(&images_dir, run_start);
    let deadline = Instant::now() + Duration::from_secs(timeout_seconds);
    let mut last: Option<(PathBuf, u64)> = None;
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| CliError::new(6, format!("wait codex: {e}")))?
        {
            // Codex exited on its own. A nonzero status is only fatal if no image
            // was written — codex may exit nonzero after a successful write.
            if !status.success() && find()?.is_none() {
                let tail = stderr_tail.lock().unwrap().clone();
                let tail = tail.lines().last().unwrap_or("no stderr").to_string();
                return Err(CliError::new(6, format!("codex failed: {tail}")));
            }
            break;
        }
        // A candidate PNG present and size-steady across two consecutive polls =>
        // the write has completed; terminate codex early.
        if let Some(candidate) = find()? {
            match fs::metadata(&candidate).map(|m| m.len()) {
                Ok(size)
                    if size > 0
                        && last.as_ref().map(|(p, s)| (p, *s)) == Some((&candidate, size)) =>
                {
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                    break;
                }
                Ok(size) if size > 0 => last = Some((candidate, size)),
                _ => last = None,
            }
        } else {
            last = None;
        }
        if Instant::now() >= deadline {
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Err(CliError::new(
                6,
                format!("codex-image timed out after {timeout_seconds}s"),
            ));
        }
        sleep(Duration::from_millis(400)).await;
    }
    // The image this run produced: the newest PNG in `generated_images` modified
    // at/after launch. The validation below still gates what we copy out.
    let generated = newest_generated_png(&images_dir, run_start)?.ok_or_else(|| {
        CliError::new(
            6,
            "codex completed but did not produce an image in generated_images",
        )
    })?;
    ctx.vlog(format!("selected generated image: {}", generated.display()));

    // Validate the bytes before trusting them: PNG signature, a plausible size,
    // and a successful decode. We copy these exact bytes to --out, so every
    // property we report downstream must actually hold here.
    let bytes = fs::read(&generated)?;
    const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    if !bytes.starts_with(&PNG_MAGIC) {
        return Err(CliError::new(
            7,
            "generated file is not a PNG (missing PNG signature)",
        ));
    }
    // 67 bytes is the smallest a structurally valid PNG can be; less is junk.
    if bytes.len() < 67 {
        return Err(CliError::new(
            7,
            format!("generated PNG is implausibly small ({} bytes)", bytes.len()),
        ));
    }
    let generated_img = image::load_from_memory(&bytes)
        .map_err(|e| CliError::new(7, format!("generated file is not a decodable image: {e}")))?;
    ctx.vlog(format!(
        "validated PNG: {} bytes, {}x{}",
        bytes.len(),
        generated_img.width(),
        generated_img.height()
    ));
    if let Some((expected_w, expected_h)) = requested_size {
        if generated_img.width() != expected_w || generated_img.height() != expected_h {
            ctx.event(
                "warning",
                json!({
                    "message": format!(
                        "provider returned {}x{} but --size requested {}x{}",
                        generated_img.width(),
                        generated_img.height(),
                        expected_w,
                        expected_h
                    ),
                    "expected_width": expected_w,
                    "expected_height": expected_h,
                    "actual_width": generated_img.width(),
                    "actual_height": generated_img.height()
                }),
            );
        }
    }
    if let Some(key) = green_key {
        let rgba = generated_img.to_rgba8();
        if !green_source_has_key_background(&rgba, key) {
            ctx.event(
                "warning",
                json!({
                    "message": "generated green-source image does not appear to contain a solid key-color background"
                }),
            );
        }
    } else if image_is_uniform(&generated_img) {
        // A full-frame single flat color usually means a blank or failed render.
        // (Green-source legitimately has flat regions, so it is excluded above.)
        ctx.event(
            "warning",
            json!({
                "message": "generated image is a single flat color; it may be blank or a failed render"
            }),
        );
    }
    write_atomic(out, &bytes, overwrite)?;
    // We own this file (it is this run's output), so move rather than copy: remove
    // it from the shared `generated_images` dir now that it is safely at --out.
    // Best-effort — a leftover is harmless (future runs filter by mtime).
    if let Err(e) = fs::remove_file(&generated) {
        ctx.vlog(format!(
            "could not remove source {}: {e}",
            generated.display()
        ));
    }
    if let Some(meta) = metadata_out {
        write_json_atomic(
            meta,
            &json!({
                "provider": "codex-image",
                "timeout_seconds": timeout_seconds,
                "sandbox": sandbox,
                "stderr": stderr_tail.lock().unwrap().clone(),
            }),
            true,
        )?;
    }
    ctx.event(
        "artifact",
        json!({"path": out, "kind": "image", "bytes": bytes.len()}),
    );
    Ok(())
}

fn image_instruction(
    kind: &str,
    prompt: &str,
    style: Option<&str>,
    size: Option<(u32, u32)>,
    green: bool,
    key: &str,
    out: &Path,
) -> String {
    let mut s = String::new();
    s.push_str("$imagegen\n");
    s.push_str("Generate a production-ready 2D game asset.\n");
    s.push_str(&format!("Asset kind: {kind}.\n"));
    // Output-target constraint derived from --out: the deliverable's file name
    // and image format. This describes the asset to produce; it does NOT change
    // the codex working-file rules appended later (which still write `asset.png`
    // into the sandbox temp dir).
    let out_name = out.file_name().unwrap_or(out.as_os_str()).to_string_lossy();
    let fmt = out
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_uppercase())
        .unwrap_or_else(|| "PNG".to_string());
    s.push_str(&format!(
        "Output target: the final deliverable is `{out_name}`, a {fmt} image. Compose for this exact format and aspect.\n"
    ));
    if let Some((w, h)) = size {
        s.push_str(&format!("Requested canvas: {w}x{h}.\n"));
    }
    if let Some(style) = style {
        s.push_str("Style constraints:\n");
        s.push_str(style);
        s.push('\n');
    }
    if green {
        s.push_str(&format!("Green-screen constraint: isolate exactly one asset on a flat pure {key} background. No gradients, no cast shadows, no scene background, no extra UI elements.\n"));
    }
    s.push_str("User brief:\n");
    s.push_str(prompt);
    s
}

fn render_sfx_file(
    out: &Path,
    preset: SfxPreset,
    duration_ms: u32,
    pitch: Option<f32>,
    seed: u64,
    sample_rate: u32,
    overwrite: bool,
) -> Result<()> {
    let tmp = temp_path_for(out);
    let result: Result<()> = (|| {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&tmp, spec)
            .map_err(|e| CliError::new(1, format!("create wav: {e}")))?;
        let n = ((duration_ms as f32 / 1000.0) * sample_rate as f32) as usize;
        let mut rng = XorShift64(seed ^ 0x9e3779b97f4a7c15);
        // Tonal presets (coin/jump/laser/...) are pure functions of time and never
        // touch `rng`, so without this every `--variations` file would be identical.
        // Derive a small deterministic detune from the seed and scale the time base
        // by it; seed 0 maps to exactly 1.0 so the canonical sound is unchanged.
        let detune = if seed == 0 {
            1.0
        } else {
            let mut jr = XorShift64(seed.wrapping_mul(0xD1B54A32D192ED03) ^ 0x94D049BB133111EB);
            1.0 + (jr.next_f32() - 0.5) * 0.06
        };
        for i in 0..n {
            let t = (i as f32 / sample_rate as f32) * detune;
            let u = i as f32 / n.max(1) as f32;
            let sample = match preset {
                SfxPreset::Coin => {
                    let f = pitch.unwrap_or(if u < 0.45 { 880.0 } else { 1320.0 });
                    sine(f, t) * decay(u, 8.0)
                }
                SfxPreset::Click => noise(&mut rng) * decay(u, 40.0) * 0.45,
                SfxPreset::Confirm => (sine(660.0, t) + 0.6 * sine(990.0, t)) * decay(u, 5.5),
                SfxPreset::Cancel => sine(240.0 + 120.0 * (1.0 - u), t) * decay(u, 8.0),
                SfxPreset::Powerup => sine(420.0 + 900.0 * u, t) * (1.0 - u).powf(0.4),
                SfxPreset::Error => square(180.0, t) * decay(u, 3.5) * 0.35,
                SfxPreset::Hit => (noise(&mut rng) * 0.45 + sine(90.0, t) * 0.8) * decay(u, 16.0),
                SfxPreset::Explosion => {
                    (noise(&mut rng) * (1.0 - u) + sine(70.0 - 30.0 * u, t)) * decay(u, 5.0)
                }
                SfxPreset::Jump => sine(280.0 + 420.0 * u, t) * decay(u, 3.0),
                SfxPreset::Laser => saw(1200.0 - 700.0 * u, t) * decay(u, 7.0) * 0.45,
                SfxPreset::Whoosh => noise(&mut rng) * (1.0 - (2.0 * u - 1.0).abs()) * 0.55,
            };
            let sample = soft_clip(sample * 0.8);
            writer
                .write_sample((sample * i16::MAX as f32) as i16)
                .map_err(|e| CliError::new(1, e.to_string()))?;
        }
        writer
            .finalize()
            .map_err(|e| CliError::new(1, e.to_string()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result?;
    let result = write_atomic_from_file(&tmp, out, overwrite);
    let _ = fs::remove_file(tmp);
    result
}

fn warn_format_extension(ctx: &Ctx, out: &Path, format: &str) {
    let ext = out
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    if !ext.is_empty() && ext != format.to_ascii_lowercase() {
        ctx.event(
            "warning",
            json!({
                "message": format!(
                    "output extension '.{ext}' does not match --format '{format}'; \
            the file will contain {format}-encoded audio"
                )
            }),
        );
    }
}

fn require_wav_extension(path: &Path) -> Result<()> {
    let ext = path
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "wav" {
        return Ok(());
    }
    Err(CliError::new(
        3,
        format!(
            "trim/waveform require a PCM WAV input; got '{}'. Convert to WAV first \
(compressed formats like mp3/ogg are not decoded by this tool).",
            if ext.is_empty() { "no extension" } else { &ext }
        ),
    ))
}

/// Read any WAV (8/16/24/32-bit PCM or 32-bit float) into samples normalized to
/// roughly [-1.0, 1.0], so downstream rendering is independent of bit depth.
fn read_wav_normalized(path: &Path) -> Result<Vec<f32>> {
    let mut reader =
        hound::WavReader::open(path).map_err(|e| CliError::new(3, format!("open wav: {e}")))?;
    let spec = reader.spec();
    match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| CliError::new(3, format!("read wav samples: {e}"))),
        hound::SampleFormat::Int => {
            let max = if spec.bits_per_sample >= 32 {
                i32::MAX as f32
            } else {
                ((1i64 << (spec.bits_per_sample - 1)) - 1) as f32
            };
            let max = max.max(1.0);
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / max))
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| CliError::new(3, format!("read wav samples: {e}")))
        }
    }
}

fn read_prompt(path: Option<&Path>, text: Option<&str>) -> Result<String> {
    match (path, text) {
        (Some(_), Some(_)) => Err(CliError::new(
            2,
            "use either --prompt or --prompt-text, not both",
        )),
        (Some(path), None) => {
            let content = fs::read_to_string(path)
                .map_err(|e| CliError::new(3, format!("{}: {e}", path.display())))?;
            if content.trim().is_empty() {
                return Err(CliError::new(
                    2,
                    format!("prompt file is empty: {}", path.display()),
                ));
            }
            Ok(content)
        }
        (None, Some(text)) => {
            if text.trim().is_empty() {
                return Err(CliError::new(2, "prompt is empty"));
            }
            Ok(text.to_string())
        }
        (None, None) => Err(CliError::new(2, "prompt is required")),
    }
}

fn read_optional(path: Option<&Path>) -> Result<Option<String>> {
    path.map(fs::read_to_string)
        .transpose()
        .map_err(|e| CliError::new(3, e.to_string()))
}

fn check_output_available(path: &Path, overwrite: bool) -> Result<()> {
    if path.exists() && !overwrite {
        return Err(CliError::new(
            4,
            format!("output exists: {}", path.display()),
        ));
    }
    Ok(())
}

fn ensure_output(path: &Path, overwrite: bool) -> Result<()> {
    check_output_available(path, overwrite)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn validate_positive_u32(name: &str, value: u32) -> Result<()> {
    if value == 0 {
        return Err(CliError::new(2, format!("{name} must be greater than 0")));
    }
    Ok(())
}

fn validate_positive_f32(name: &str, value: f32) -> Result<()> {
    if !value.is_finite() || value <= 0.0 {
        return Err(CliError::new(2, format!("{name} must be greater than 0")));
    }
    Ok(())
}

fn validate_nonnegative_f32(name: &str, value: f32) -> Result<()> {
    if !value.is_finite() || value < 0.0 {
        return Err(CliError::new(2, format!("{name} must be non-negative")));
    }
    Ok(())
}

fn validate_timeout(seconds: u64) -> Result<()> {
    if seconds == 0 {
        return Err(CliError::new(2, "--timeout-seconds must be greater than 0"));
    }
    Ok(())
}

// 768 kHz comfortably covers any real audio workflow while keeping the WAV
// byte-rate (sample_rate * channels * bytes_per_sample) far below the u32 ceiling
// that `hound` multiplies into, which otherwise panics on absurd values.
const MAX_SAMPLE_RATE: u32 = 768_000;

fn validate_sample_rate(value: u32) -> Result<()> {
    validate_positive_u32("--sample-rate", value)?;
    if value > MAX_SAMPLE_RATE {
        return Err(CliError::new(
            2,
            format!("--sample-rate must be between 1 and {MAX_SAMPLE_RATE}"),
        ));
    }
    Ok(())
}

// Cap each canvas axis so a huge cols/cell (or frame count) can't overflow the
// u32 multiply — which panics in debug and silently wraps into a corrupt image in
// release — or balloon into a multi-gigabyte allocation.
const MAX_CANVAS_DIM: u32 = 16_384;

fn checked_canvas_dims(cols: u32, rows: u32, cell_w: u32, cell_h: u32) -> Result<(u32, u32)> {
    let width = cols
        .checked_mul(cell_w)
        .filter(|d| *d <= MAX_CANVAS_DIM)
        .ok_or_else(|| {
            CliError::new(
                2,
                format!("sheet width ({cols} x {cell_w}px) exceeds the {MAX_CANVAS_DIM}px limit"),
            )
        })?;
    let height = rows
        .checked_mul(cell_h)
        .filter(|d| *d <= MAX_CANVAS_DIM)
        .ok_or_else(|| {
            CliError::new(
                2,
                format!("sheet height ({rows} x {cell_h}px) exceeds the {MAX_CANVAS_DIM}px limit"),
            )
        })?;
    Ok((width, height))
}

fn warn_if_alpha_dropping_format(ctx: &Ctx, out: &Path) {
    let ext = out
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(ext.as_str(), "jpg" | "jpeg") {
        ctx.event(
            "warning",
            json!({
                "message": format!(
                    "output '.{ext}' cannot store transparency; the alpha channel will be \
            discarded. Use a .png output to preserve transparency."
                )
            }),
        );
    }
}

/// Detect a chroma key color from the corners of the first frame for `--key auto`:
/// pick the corner color shared by the most corners (ties resolve to top-left).
fn detect_key_color(files: &[PathBuf]) -> Result<[u8; 3]> {
    let first = files
        .first()
        .ok_or_else(|| CliError::new(6, "no frames available to auto-detect a key color"))?;
    let img = image::open(first)
        .map_err(|e| CliError::new(3, format!("{}: {e}", first.display())))?
        .to_rgba8();
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return Err(CliError::new(3, "first frame is empty"));
    }
    let corners = [
        img.get_pixel(0, 0).0,
        img.get_pixel(w - 1, 0).0,
        img.get_pixel(0, h - 1).0,
        img.get_pixel(w - 1, h - 1).0,
    ];
    let mut best = corners[0];
    let mut best_count = 0;
    for c in &corners {
        let key = [c[0], c[1], c[2]];
        let count = corners
            .iter()
            .filter(|o| color_distance(**o, key) <= 24.0)
            .count();
        if count > best_count {
            best_count = count;
            best = *c;
        }
    }
    Ok([best[0], best[1], best[2]])
}

/// Minimal POSIX shell quoting for displaying a command line. Bare if the token
/// is a simple word; single-quoted (with embedded quotes escaped) otherwise.
fn shell_quote(s: &str) -> String {
    let safe = !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_./=:,@%+".contains(&b));
    if safe {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

fn codex_sandbox() -> Result<String> {
    let sandbox = env::var("CODEX_SANDBOX").unwrap_or_else(|_| "workspace-write".into());
    match sandbox.as_str() {
        // danger-full-access is opt-in only (never the default): it removes the
        // codex sandbox entirely, so isolation then rests solely on the private
        // 0700 temp working dir we pass via `-C`.
        "workspace-write" | "read-only" | "danger-full-access" => Ok(sandbox),
        _ => Err(CliError::new(
            2,
            "CODEX_SANDBOX must be workspace-write, read-only, or danger-full-access",
        )),
    }
}

fn write_atomic(path: &Path, bytes: &[u8], overwrite: bool) -> Result<()> {
    ensure_output(path, overwrite)?;
    let tmp = temp_path_for(path);
    fs::write(&tmp, bytes).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        CliError::from(e)
    })?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        CliError::from(e)
    })?;
    Ok(())
}

fn write_atomic_from_file(src: &Path, dst: &Path, overwrite: bool) -> Result<()> {
    ensure_output(dst, overwrite)?;
    let bytes = fs::read(src)?;
    write_atomic(dst, &bytes, true)
}

fn write_json_atomic(path: &Path, value: &Value, overwrite: bool) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| CliError::new(1, e.to_string()))?;
    write_atomic(path, &bytes, overwrite)
}

fn save_image_atomic(path: &Path, img: &DynamicImage, overwrite: bool) -> Result<()> {
    ensure_output(path, overwrite)?;
    let tmp = temp_path_for(path);
    img.save(&tmp).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        CliError::new(1, format!("save image: {e}"))
    })?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        CliError::from(e)
    })?;
    Ok(())
}

fn save_rgba_atomic(path: &Path, img: &RgbaImage, overwrite: bool) -> Result<()> {
    save_image_atomic(path, &DynamicImage::ImageRgba8(img.clone()), overwrite)
}

fn temp_path_for(path: &Path) -> PathBuf {
    let mut p = path.to_path_buf();
    let stem = path.file_stem().and_then(OsStr::to_str).unwrap_or("out");
    let ext = path.extension().and_then(OsStr::to_str);
    let name = match ext {
        Some(ext) => format!(".{stem}.{}.{}", std::process::id(), ext),
        None => format!(".{stem}.{}.tmp", std::process::id()),
    };
    p.set_file_name(name);
    p
}

fn parse_size(s: &str) -> Result<(u32, u32)> {
    let (w, h) = s
        .split_once('x')
        .ok_or_else(|| CliError::new(2, "size must be WIDTHxHEIGHT"))?;
    let w = w.parse().map_err(|_| CliError::new(2, "invalid width"))?;
    let h = h.parse().map_err(|_| CliError::new(2, "invalid height"))?;
    validate_positive_u32("width", w)?;
    validate_positive_u32("height", h)?;
    Ok((w, h))
}

fn parse_box(s: &str) -> Result<(u32, u32, u32, u32)> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != 4 {
        return Err(CliError::new(2, "box must be x,y,w,h"));
    }
    let x = parts[0]
        .parse()
        .map_err(|_| CliError::new(2, "invalid box x"))?;
    let y = parts[1]
        .parse()
        .map_err(|_| CliError::new(2, "invalid box y"))?;
    let w = parts[2]
        .parse()
        .map_err(|_| CliError::new(2, "invalid box w"))?;
    let h = parts[3]
        .parse()
        .map_err(|_| CliError::new(2, "invalid box h"))?;
    validate_positive_u32("box w", w)?;
    validate_positive_u32("box h", h)?;
    Ok((x, y, w, h))
}

fn parse_grid(s: &str) -> Result<(u32, u32)> {
    let (cols, rows) = s
        .split_once('x')
        .ok_or_else(|| CliError::new(2, "grid must be COLSxROWS"))?;
    let cols = cols
        .parse()
        .map_err(|_| CliError::new(2, "invalid grid cols"))?;
    let rows = rows
        .parse()
        .map_err(|_| CliError::new(2, "invalid grid rows"))?;
    if cols == 0 || rows == 0 {
        return Err(CliError::new(2, "grid dimensions must be non-zero"));
    }
    Ok((cols, rows))
}

fn parse_hex_color(s: &str) -> Result<[u8; 3]> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 {
        return Err(CliError::new(2, "color must be #RRGGBB"));
    }
    let bytes = hex::decode(h).map_err(|_| CliError::new(2, "invalid hex color"))?;
    Ok([bytes[0], bytes[1], bytes[2]])
}

/// Reduce key-color spill on a kept pixel, but only when that pixel is plausibly
/// contaminated by the key — i.e. it sits within a spill band just outside the
/// keying tolerance. Pixels far from the key hue (legitimate teal/green art) are
/// left untouched, so despill no longer discolors the whole image.
fn apply_despill(pixel: &mut [u8; 4], key: [u8; 3], distance: f32, tolerance: f32, spill: f32) {
    if spill <= 0.0 {
        return;
    }
    // Spill band: from the keying edge outward, scaled by tolerance. Anchoring
    // the band to tolerance (rather than a fixed 32px floor) means tolerance=0
    // keys nothing AND despills nothing, so solid key-color pixels the user
    // chose to keep are left untouched instead of being silently desaturated.
    let band = tolerance * 2.0;
    if distance > band {
        return;
    }
    // Identify the key's dominant channel and only suppress that channel.
    let key_max = key[0].max(key[1]).max(key[2]);
    if key_max == 0 {
        return;
    }
    let chan = if key[1] == key_max {
        1
    } else if key[0] == key_max {
        0
    } else {
        2
    };
    let c = pixel[chan] as f32;
    let other = match chan {
        1 => pixel[0].max(pixel[2]),
        0 => pixel[1].max(pixel[2]),
        _ => pixel[0].max(pixel[1]),
    } as f32;
    if c > other {
        pixel[chan] = (c - (c - other) * spill).clamp(0.0, 255.0) as u8;
    }
}

fn color_distance(pixel: [u8; 4], key: [u8; 3]) -> f32 {
    let dr = pixel[0] as f32 - key[0] as f32;
    let dg = pixel[1] as f32 - key[1] as f32;
    let db = pixel[2] as f32 - key[2] as f32;
    (dr * dr + dg * dg + db * db).sqrt()
}

fn trim_alpha(img: &RgbaImage) -> Option<RgbaImage> {
    let mut min_x = img.width();
    let mut min_y = img.height();
    let mut max_x = 0;
    let mut max_y = 0;
    let mut found = false;
    for (x, y, p) in img.enumerate_pixels() {
        if p.0[3] > 0 {
            found = true;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    if !found {
        return None;
    }
    Some(imageops::crop_imm(img, min_x, min_y, max_x - min_x + 1, max_y - min_y + 1).to_image())
}

fn green_source_has_key_background(img: &RgbaImage, key: [u8; 3]) -> bool {
    if img.width() == 0 || img.height() == 0 {
        return false;
    }
    // Allow for mild provider/compression noise rather than requiring an exact match.
    const KEY_TOLERANCE: f32 = 24.0;
    let corners = [
        img.get_pixel(0, 0).0,
        img.get_pixel(img.width() - 1, 0).0,
        img.get_pixel(0, img.height() - 1).0,
        img.get_pixel(img.width() - 1, img.height() - 1).0,
    ];
    if corners
        .iter()
        .all(|p| color_distance(*p, key) <= KEY_TOLERANCE)
    {
        return true;
    }
    let keyed = img
        .pixels()
        .filter(|p| color_distance(p.0, key) <= KEY_TOLERANCE)
        .count();
    keyed as f32 / (img.width() as f32 * img.height() as f32) >= 0.05
}

fn anchor_offset(anchor: Anchor, cw: u32, ch: u32, iw: u32, ih: u32) -> (u32, u32) {
    match anchor {
        Anchor::Center => ((cw.saturating_sub(iw)) / 2, (ch.saturating_sub(ih)) / 2),
        Anchor::TopLeft => (0, 0),
        Anchor::BottomCenter => ((cw.saturating_sub(iw)) / 2, ch.saturating_sub(ih)),
    }
}

fn sine(freq: f32, t: f32) -> f32 {
    (std::f32::consts::TAU * freq * t).sin()
}

fn square(freq: f32, t: f32) -> f32 {
    if sine(freq, t) >= 0.0 {
        1.0
    } else {
        -1.0
    }
}

fn saw(freq: f32, t: f32) -> f32 {
    2.0 * ((freq * t).fract()) - 1.0
}

fn decay(u: f32, k: f32) -> f32 {
    (-k * u).exp()
}

fn soft_clip(x: f32) -> f32 {
    x / (1.0 + x.abs())
}

fn noise(rng: &mut XorShift64) -> f32 {
    rng.next_f32() * 2.0 - 1.0
}

struct XorShift64(u64);

impl XorShift64 {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn next_f32(&mut self) -> f32 {
        (self.next_u64() as f64 / u64::MAX as f64) as f32
    }
}

fn preset_name(p: SfxPreset) -> &'static str {
    match p {
        SfxPreset::Click => "click",
        SfxPreset::Confirm => "confirm",
        SfxPreset::Cancel => "cancel",
        SfxPreset::Coin => "coin",
        SfxPreset::Powerup => "powerup",
        SfxPreset::Error => "error",
        SfxPreset::Hit => "hit",
        SfxPreset::Explosion => "explosion",
        SfxPreset::Jump => "jump",
        SfxPreset::Laser => "laser",
        SfxPreset::Whoosh => "whoosh",
    }
}

fn kind_from_ext(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" | "jpg" | "jpeg" | "webp" => "image",
        "wav" | "mp3" | "ogg" | "m4a" => "audio",
        "json" | "yaml" | "yml" => "metadata",
        "mp4" | "mov" | "webm" => "video",
        _ => "file",
    }
}

/// True if every pixel is identical (a single flat color). Used to flag blank or
/// failed renders. A zero-pixel image counts as uniform.
fn image_is_uniform(img: &DynamicImage) -> bool {
    let rgba = img.to_rgba8();
    let mut pixels = rgba.pixels();
    match pixels.next() {
        Some(&first) => pixels.all(|&p| p == first),
        None => true,
    }
}

/// All `.png` files anywhere under `dir` (recursive). Used to locate the codex
/// output even when it lands in a subdirectory of the sandbox workdir.
fn find_pngs_recursive(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries = match fs::read_dir(&d) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(ft) if ft.is_dir() => stack.push(path),
                Ok(ft) if ft.is_file() => {
                    let is_png = path
                        .extension()
                        .and_then(OsStr::to_str)
                        .map(|e| e.eq_ignore_ascii_case("png"))
                        .unwrap_or(false);
                    if is_png {
                        out.push(path);
                    }
                }
                _ => {}
            }
        }
    }
    out.sort();
    Ok(out)
}

/// `$CODEX_HOME/generated_images` (or `$HOME/.codex/generated_images`). codex's
/// imagegen always writes its PNG output here. Returns the path even if it does
/// not exist yet (codex creates it on first use); None only when neither
/// CODEX_HOME nor HOME is set.
fn codex_generated_images_dir() -> Option<PathBuf> {
    let home = env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".codex")))?;
    Some(home.join("generated_images"))
}

/// The newest PNG in `dir` modified at/after `after`. The freshness filter is
/// essential: `generated_images` accumulates images across sessions, so without
/// it we could pick up an unrelated image from a previous run. None if no fresh
/// PNG exists (or `dir` does not exist yet).
fn newest_generated_png(dir: &Path, after: SystemTime) -> Result<Option<PathBuf>> {
    let mut fresh: Vec<PathBuf> = find_pngs_recursive(dir)?
        .into_iter()
        .filter(|p| {
            fs::metadata(p)
                .and_then(|m| m.modified())
                .map(|t| t >= after)
                .unwrap_or(false)
        })
        .collect();
    fresh.sort_by_key(|p| fs::metadata(p).and_then(|m| m.modified()).ok());
    Ok(fresh.into_iter().next_back())
}

fn sorted_pngs(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .and_then(OsStr::to_str)
                .map(|e| e.eq_ignore_ascii_case("png"))
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    Ok(files)
}

/// Remove `frame_*.png` files this tool generated in a prior run so `--overwrite`
/// is a clean replacement. Only touches files matching our own naming scheme.
fn clear_frame_pngs(dir: &Path) -> Result<()> {
    for file in sorted_pngs(dir)? {
        let is_frame = file
            .file_name()
            .and_then(OsStr::to_str)
            .map(|n| n.starts_with("frame_"))
            .unwrap_or(false);
        if is_frame {
            fs::remove_file(&file)?;
        }
    }
    Ok(())
}

fn image_kind_name(kind: &ImageKind) -> &'static str {
    match kind {
        ImageKind::Scene => "scene",
        ImageKind::Concept => "concept",
        ImageKind::Background => "background",
        ImageKind::Character => "character",
        ImageKind::Sprite => "sprite",
        ImageKind::Ui => "ui",
        ImageKind::Icon => "icon",
        ImageKind::Logo => "logo",
        ImageKind::Effect => "effect",
        ImageKind::Frames => "frames",
        ImageKind::Map => "map",
        ImageKind::Tile => "tile",
    }
}

fn green_kind_name(kind: &GreenKind) -> &'static str {
    match kind {
        GreenKind::Button => "button",
        GreenKind::Panel => "panel",
        GreenKind::Icon => "icon",
        GreenKind::Bar => "bar",
        GreenKind::Character => "character",
        GreenKind::Prop => "prop",
        GreenKind::Effect => "effect",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_generated_png_ignores_stale_and_picks_fresh() {
        let dir = TempDir::new().unwrap();
        // A stale image from a "previous session" must be ignored.
        let stale = dir.path().join("old.png");
        fs::write(&stale, b"old").unwrap();

        let cutoff = SystemTime::now();
        // Nothing fresh yet => None (the stale file predates the cutoff).
        assert_eq!(newest_generated_png(dir.path(), cutoff).unwrap(), None);

        // This run's image appears after the cutoff, even nested in a subdir.
        let sub = dir.path().join("sub");
        fs::create_dir_all(&sub).unwrap();
        let fresh = sub.join("new.png");
        fs::write(&fresh, b"new").unwrap();
        assert_eq!(
            newest_generated_png(dir.path(), cutoff).unwrap(),
            Some(fresh)
        );
    }

    #[test]
    fn image_instruction_always_includes_imagegen_keyword() {
        let plain = image_instruction(
            "scene",
            "a cat",
            None,
            None,
            false,
            "#00FF00",
            Path::new("hero/cat.png"),
        );
        assert!(plain.contains("$imagegen"), "plain: {plain}");
        assert!(plain.contains("cat.png"), "plain out target: {plain}");
        let green = image_instruction(
            "button",
            "a coin",
            None,
            None,
            true,
            "#00FF00",
            Path::new("ui/coin.png"),
        );
        assert!(green.contains("$imagegen"), "green: {green}");
    }

    #[test]
    fn image_is_uniform_detects_flat_vs_varied() {
        let flat = DynamicImage::ImageRgba8(RgbaImage::from_pixel(4, 4, Rgba([10, 20, 30, 255])));
        assert!(image_is_uniform(&flat));
        let mut varied = RgbaImage::from_pixel(4, 4, Rgba([10, 20, 30, 255]));
        varied.put_pixel(2, 1, Rgba([11, 20, 30, 255]));
        assert!(!image_is_uniform(&DynamicImage::ImageRgba8(varied)));
    }

    #[test]
    fn parses_size() {
        assert_eq!(parse_size("1280x720").unwrap(), (1280, 720));
        assert!(parse_size("1280,720").is_err());
        assert!(parse_size("0x720").is_err());
    }

    #[test]
    fn parses_box() {
        assert_eq!(parse_box("1,2,3,4").unwrap(), (1, 2, 3, 4));
        assert!(parse_box("1,2,3").is_err());
        assert!(parse_box("1,2,0,4").is_err());
    }

    #[test]
    fn parses_grid() {
        assert_eq!(parse_grid("8x2").unwrap(), (8, 2));
        assert!(parse_grid("0x2").is_err());
    }

    #[test]
    fn version_comparison_orders_releases() {
        assert!(version_is_newer("0.2.0", "0.1.0"));
        assert!(version_is_newer("1.0.0", "0.9.9"));
        assert!(version_is_newer("0.1.10", "0.1.2"));
        assert!(!version_is_newer("0.1.0", "0.1.0"));
        assert!(!version_is_newer("0.1.0", "0.2.0"));
        // A leading-v tag is stripped by the caller before comparison.
        assert!(version_is_newer("0.1.0".trim_start_matches('v'), "0.0.9"));
    }

    #[test]
    fn current_target_is_known_for_this_platform() {
        // The build only runs on supported hosts, so this must resolve a triple.
        let target = current_target().unwrap();
        assert!(target.contains('-'));
    }

    #[test]
    fn trims_alpha() {
        let mut img = RgbaImage::from_pixel(4, 4, Rgba([0, 0, 0, 0]));
        img.put_pixel(1, 2, Rgba([255, 255, 255, 255]));
        let out = trim_alpha(&img).unwrap();
        assert_eq!(out.dimensions(), (1, 1));
    }

    #[test]
    fn detects_green_source_background() {
        let green = RgbaImage::from_pixel(8, 8, Rgba([0, 255, 0, 255]));
        assert!(green_source_has_key_background(&green, [0, 255, 0]));

        let red = RgbaImage::from_pixel(8, 8, Rgba([255, 0, 0, 255]));
        assert!(!green_source_has_key_background(&red, [0, 255, 0]));
    }

    #[test]
    fn resolves_absolute_executable_paths() {
        let current = std::env::current_exe().unwrap();
        assert_eq!(
            resolve_executable(current.to_str().unwrap()).unwrap(),
            current
        );
        assert!(resolve_executable("/definitely/missing/game-asset").is_none());
    }

    #[test]
    fn rejects_small_contact_sheet_cell() {
        let dir = TempDir::new().unwrap();
        let input = dir.path().join("input.png");
        let output = dir.path().join("contact.png");
        save_rgba_atomic(
            &input,
            &RgbaImage::from_pixel(16, 16, Rgba([255, 0, 0, 255])),
            false,
        )
        .unwrap();
        let err = contact_sheet(
            &Ctx {
                json: false,
                quiet: true,
                include_prompts: false,
                verbose: false,
                pending_start: Mutex::new(None),
            },
            ContactSheetArgs {
                inputs: vec![input.display().to_string()],
                out: output,
                cols: 1,
                cell: 8,
                overwrite: false,
            },
        )
        .unwrap_err();
        assert_eq!(err.code, 2);
    }

    #[test]
    fn rejects_invalid_sfx_parameters() {
        let dir = TempDir::new().unwrap();
        let err = audio_sfx(
            &Ctx {
                json: false,
                quiet: true,
                include_prompts: false,
                verbose: false,
                pending_start: Mutex::new(None),
            },
            AudioSfxArgs {
                preset: SfxPreset::Coin,
                duration_ms: 260,
                pitch: None,
                seed: 0,
                sample_rate: 0,
                out: Some(dir.path().join("bad.wav")),
                out_dir: None,
                variations: 1,
                overwrite: false,
            },
        )
        .unwrap_err();
        assert_eq!(err.code, 2);
    }

    #[test]
    fn rejects_zero_waveform_dimensions_without_temp_file() {
        let dir = TempDir::new().unwrap();
        let input = dir.path().join("input.wav");
        let output = dir.path().join("wave.png");
        render_sfx_file(&input, SfxPreset::Coin, 100, None, 0, 44100, false).unwrap();
        let err = audio_waveform(
            &Ctx {
                json: false,
                quiet: true,
                include_prompts: false,
                verbose: false,
                pending_start: Mutex::new(None),
            },
            AudioWaveformArgs {
                input,
                out: output.clone(),
                width: 0,
                height: 120,
                overwrite: false,
            },
        )
        .unwrap_err();
        assert_eq!(err.code, 2);
        assert!(!temp_path_for(&output).exists());
    }

    #[test]
    fn despill_leaves_far_colors_untouched() {
        // A teal pixel far from pure green must not be desaturated.
        let mut teal = [0u8, 200, 180, 255];
        let key = [0u8, 255, 0];
        let d = color_distance(teal, key);
        apply_despill(&mut teal, key, d, 42.0, 0.75);
        assert_eq!(teal, [0, 200, 180, 255]);

        // A near-key spill pixel (light green tint) does get its green pulled down.
        let mut spill = [40u8, 230, 40, 255];
        let d = color_distance(spill, key);
        apply_despill(&mut spill, key, d, 42.0, 0.75);
        assert!(spill[1] < 230);
    }

    #[test]
    fn require_wav_extension_rejects_mp3() {
        assert!(require_wav_extension(Path::new("song.mp3")).is_err());
        assert!(require_wav_extension(Path::new("clip.wav")).is_ok());
        assert!(require_wav_extension(Path::new("CLIP.WAV")).is_ok());
    }

    #[test]
    fn trims_24bit_and_float_wav() {
        let dir = TempDir::new().unwrap();
        for (name, fmt, bits) in [
            ("p24.wav", hound::SampleFormat::Int, 24),
            ("f32.wav", hound::SampleFormat::Float, 32),
        ] {
            let path = dir.path().join(name);
            let spec = hound::WavSpec {
                channels: 1,
                sample_rate: 44100,
                bits_per_sample: bits,
                sample_format: fmt,
            };
            let mut w = hound::WavWriter::create(&path, spec).unwrap();
            for i in 0..44100 {
                match fmt {
                    hound::SampleFormat::Int => {
                        w.write_sample(i % 1000).unwrap();
                    }
                    hound::SampleFormat::Float => {
                        w.write_sample((i as f32 / 44100.0) - 0.5).unwrap();
                    }
                }
            }
            w.finalize().unwrap();
            let out = dir.path().join(format!("trim_{name}"));
            audio_trim(
                &Ctx {
                    json: false,
                    quiet: true,
                    include_prompts: false,
                    verbose: false,
                    pending_start: Mutex::new(None),
                },
                AudioTrimArgs {
                    input: path,
                    start: 0.1,
                    end: 0.3,
                    out: out.clone(),
                    overwrite: false,
                },
            )
            .unwrap();
            let r = hound::WavReader::open(&out).unwrap();
            assert_eq!(r.spec().bits_per_sample, bits);
            // ~0.2s at 44.1kHz
            assert!((r.duration() as i64 - 8820).abs() < 4);
        }
    }

    #[test]
    fn checked_canvas_dims_rejects_overflow() {
        // The contact-sheet / sheet-pack panic: 70000 * 70000 overflows u32.
        assert!(checked_canvas_dims(70_000, 70_000, 70_000, 70_000).is_err());
        assert!(checked_canvas_dims(1, 1, u32::MAX, 1).is_err());
        // A sane sheet still passes.
        assert_eq!(checked_canvas_dims(6, 3, 160, 160).unwrap(), (960, 480));
    }

    #[test]
    fn validate_sample_rate_bounds() {
        assert!(validate_sample_rate(0).is_err());
        assert!(validate_sample_rate(u32::MAX).is_err());
        assert!(validate_sample_rate(44_100).is_ok());
        assert!(validate_sample_rate(MAX_SAMPLE_RATE).is_ok());
    }

    #[test]
    fn detect_key_color_picks_majority_corner() {
        let dir = TempDir::new().unwrap();
        // Three green corners, one red: auto should pick green.
        let mut img = RgbaImage::from_pixel(8, 8, Rgba([0, 255, 0, 255]));
        img.put_pixel(7, 7, Rgba([255, 0, 0, 255]));
        let path = dir.path().join("frame_000.png");
        save_rgba_atomic(&path, &img, false).unwrap();
        assert_eq!(detect_key_color(&[path]).unwrap(), [0, 255, 0]);
    }

    #[test]
    fn sfx_variation_seed_does_not_overflow() {
        // --seed u64::MAX with >1 variation used to overflow on the +i.
        let dir = TempDir::new().unwrap();
        audio_sfx(
            &Ctx {
                json: false,
                quiet: true,
                include_prompts: false,
                verbose: false,
                pending_start: Mutex::new(None),
            },
            AudioSfxArgs {
                preset: SfxPreset::Coin,
                duration_ms: 20,
                pitch: None,
                seed: u64::MAX,
                sample_rate: 44100,
                out: None,
                out_dir: Some(dir.path().join("out")),
                variations: 2,
                overwrite: false,
            },
        )
        .unwrap();
    }

    #[test]
    fn manifest_single_file_uses_file_name() {
        let dir = TempDir::new().unwrap();
        let input = dir.path().join("solo.wav");
        render_sfx_file(&input, SfxPreset::Coin, 50, None, 0, 44100, false).unwrap();
        let out = dir.path().join("m.json");
        manifest(
            &Ctx {
                json: false,
                quiet: true,
                include_prompts: false,
                verbose: false,
                pending_start: Mutex::new(None),
            },
            ManifestArgs {
                input,
                out: out.clone(),
                overwrite: false,
            },
        )
        .unwrap();
        let v: Value = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
        assert_eq!(
            v.pointer("/assets/0/path").and_then(Value::as_str),
            Some("solo.wav")
        );
    }

    #[test]
    fn trim_start_beyond_input_errors_instead_of_empty_wav() {
        // A --start past the end of the input used to write a 0-sample WAV and
        // report success; it must now fail with exit code 2 and write nothing.
        let dir = TempDir::new().unwrap();
        let input = dir.path().join("short.wav"); // ~0.5s mono
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 44100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&input, spec).unwrap();
        for i in 0..22050 {
            w.write_sample((i % 1000) as i16).unwrap();
        }
        w.finalize().unwrap();
        let out = dir.path().join("trim.wav");
        let err = audio_trim(
            &Ctx {
                json: false,
                quiet: true,
                include_prompts: false,
                verbose: false,
                pending_start: Mutex::new(None),
            },
            AudioTrimArgs {
                input,
                start: 5.0,
                end: 6.0,
                out: out.clone(),
                overwrite: false,
            },
        )
        .unwrap_err();
        assert_eq!(err.code, 2);
        assert!(!out.exists(), "no output file should be written");
    }

    #[test]
    fn read_prompt_rejects_empty_and_whitespace_text() {
        assert!(read_prompt(None, Some("")).is_err());
        assert!(read_prompt(None, Some("   \n\t ")).is_err());
        assert_eq!(read_prompt(None, Some("")).unwrap_err().code, 2);
        // A real prompt still passes through verbatim.
        assert_eq!(read_prompt(None, Some(" hi ")).unwrap(), " hi ");
    }

    #[test]
    fn read_prompt_rejects_empty_file() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("empty.md");
        fs::write(&p, "   \n").unwrap();
        let err = read_prompt(Some(&p), None).unwrap_err();
        assert_eq!(err.code, 2);
    }

    #[test]
    fn doctor_respects_quiet() {
        // --quiet doctor must emit nothing on stdout/stderr and still exit Ok.
        doctor(&Ctx {
            json: false,
            quiet: true,
            include_prompts: false,
            verbose: false,
            pending_start: Mutex::new(None),
        })
        .unwrap();
    }
}
