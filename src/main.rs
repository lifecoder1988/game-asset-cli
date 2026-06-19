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
    time::Instant,
};
use tempfile::TempDir;

#[derive(Parser)]
#[command(name = "game-asset")]
#[command(version)]
#[command(about = "Stateless game asset generation and post-processing CLI")]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[arg(long, global = true)]
    quiet: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Image(ImageCmd),
    Sprite(SpriteCmd),
    Video(VideoCmd),
    Audio(AudioCmd),
    ContactSheet(ContactSheetArgs),
    Manifest(ManifestArgs),
    Doctor,
}

#[derive(Args)]
struct ImageCmd {
    #[command(subcommand)]
    command: ImageSubcommand,
}

#[derive(Subcommand)]
enum ImageSubcommand {
    Generate(ImageGenerateArgs),
    Crop(CropArgs),
    #[command(name = "green-source")]
    GreenSource(GreenSourceArgs),
    #[command(name = "chroma-key")]
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
    SheetSlice(SheetSliceArgs),
    #[command(name = "sheet-pack")]
    SheetPack(SheetPackArgs),
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
    Bgm(AudioBgmArgs),
    Sfx(AudioSfxArgs),
    Trim(AudioTrimArgs),
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
    let cli = Cli::parse();
    let started = Instant::now();
    let code = match run(cli, started).await {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("error: {}", err.message);
            err.code
        }
    };
    std::process::exit(code);
}

async fn run(cli: Cli, started: Instant) -> Result<()> {
    let ctx = Ctx {
        json: cli.json,
        quiet: cli.quiet,
    };
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
    }
    ctx.event("done", json!({"elapsed_ms": started.elapsed().as_millis()}));
    Ok(())
}

struct Ctx {
    json: bool,
    quiet: bool,
}

impl Ctx {
    fn event(&self, typ: &str, value: Value) {
        if self.json {
            let mut obj = serde_json::Map::new();
            obj.insert("type".into(), Value::String(typ.into()));
            if let Value::Object(map) = value {
                for (k, v) in map {
                    obj.insert(k, v);
                }
            }
            println!("{}", Value::Object(obj));
        } else if !self.quiet && typ == "artifact" {
            if let Some(path) = value.get("path").and_then(Value::as_str) {
                eprintln!("wrote {path}");
            }
        }
    }
}

async fn image_generate(ctx: &Ctx, args: ImageGenerateArgs) -> Result<()> {
    ensure_output(&args.out, args.overwrite)?;
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
    );
    run_codex_image(
        ctx,
        &instruction,
        &args.refs,
        &args.out,
        args.overwrite,
        args.metadata_out.as_deref(),
        args.codex_model.as_deref(),
        args.dry_run,
    )
    .await
}

async fn image_green_source(ctx: &Ctx, args: GreenSourceArgs) -> Result<()> {
    ensure_output(&args.out, args.overwrite)?;
    let prompt = read_prompt(args.prompt.as_deref(), args.prompt_text.as_deref())?;
    let instruction = image_instruction(
        green_kind_name(&args.kind),
        &prompt,
        None,
        None,
        true,
        &args.key_color,
    );
    run_codex_image(
        ctx,
        &instruction,
        &args.refs,
        &args.out,
        args.overwrite,
        args.metadata_out.as_deref(),
        args.codex_model.as_deref(),
        args.dry_run,
    )
    .await
}

fn image_crop(ctx: &Ctx, args: CropArgs) -> Result<()> {
    ensure_output(&args.out, args.overwrite)?;
    let (x, y, w, h) = parse_box(&args.box_)?;
    let img =
        image::open(&args.input).map_err(|e| CliError::new(3, format!("decode image: {e}")))?;
    if x + w > img.width() || y + h > img.height() {
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
    let img =
        image::open(&args.input).map_err(|e| CliError::new(3, format!("decode image: {e}")))?;
    let mut rgba = img.to_rgba8();
    for pixel in rgba.pixels_mut() {
        let d = color_distance(pixel.0, key);
        if d <= args.tolerance {
            pixel.0[3] = 0;
        } else {
            let spill = args.despill.clamp(0.0, 1.0);
            let g = pixel.0[1] as f32;
            let max_rb = pixel.0[0].max(pixel.0[2]) as f32;
            if g > max_rb {
                pixel.0[1] = ((g - (g - max_rb) * spill).clamp(0.0, 255.0)) as u8;
            }
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
    fs::create_dir_all(&args.out_dir)?;
    let img = image::open(&args.input)
        .map_err(|e| CliError::new(3, format!("decode image: {e}")))?
        .to_rgba8();
    let cell_w = img.width() / cols;
    let cell_h = img.height() / rows;
    if cell_w == 0 || cell_h == 0 {
        return Err(CliError::new(2, "grid is larger than image"));
    }
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
    let cols = args.cols.max(1);
    let rows = (files.len() as u32 + cols - 1) / cols;
    let mut sheet = RgbaImage::from_pixel(cols * frame_w, rows * frame_h, Rgba([0, 0, 0, 0]));
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
            true,
        )?;
    }
    ctx.event("artifact", json!({"path": args.out, "kind": "image"}));
    Ok(())
}

fn video_slice(ctx: &Ctx, args: VideoSliceArgs) -> Result<()> {
    if which::which("ffmpeg").is_err() {
        return Err(CliError::new(5, "ffmpeg not found in PATH"));
    }
    if args.end <= args.start {
        return Err(CliError::new(2, "--end must be greater than --start"));
    }
    if args.out_dir.exists() && !args.overwrite {
        return Err(CliError::new(
            4,
            format!("output directory exists: {}", args.out_dir.display()),
        ));
    }
    fs::create_dir_all(&args.out_dir)?;
    let frames = args.frames.clamp(1, 64);
    let duration = args.end - args.start;
    let fps = frames as f32 / duration;
    let pattern = args.out_dir.join("frame_%03d.png");
    let status = Command::new("ffmpeg")
        .arg("-y")
        .arg("-ss")
        .arg(args.start.to_string())
        .arg("-i")
        .arg(&args.input)
        .arg("-t")
        .arg(duration.to_string())
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
    if let Some(key) = args.key.as_deref().filter(|k| *k != "auto") {
        let key = parse_hex_color(key)?;
        for file in sorted_pngs(&args.out_dir)? {
            let mut rgba = image::open(&file)
                .map_err(|e| CliError::new(3, format!("{}: {e}", file.display())))?
                .to_rgba8();
            for pixel in rgba.pixels_mut() {
                if color_distance(pixel.0, key) <= 42.0 {
                    pixel.0[3] = 0;
                }
            }
            save_rgba_atomic(&file, &rgba, true)?;
        }
    }
    for file in sorted_pngs(&args.out_dir)? {
        ctx.event("artifact", json!({"path": file, "kind": "image"}));
    }
    Ok(())
}

async fn audio_bgm(ctx: &Ctx, args: AudioBgmArgs) -> Result<()> {
    ensure_output(&args.out, args.overwrite)?;
    let prompt = read_prompt(args.prompt.as_deref(), args.prompt_text.as_deref())?;
    let lyrics = read_prompt(args.lyrics.as_deref(), args.lyrics_text.as_deref()).ok();
    if !args.instrumental && lyrics.is_none() && !args.lyrics_optimizer {
        return Err(CliError::new(
            2,
            "vocals require --lyrics/--lyrics-text or --lyrics-optimizer",
        ));
    }
    if args.dry_run {
        ctx.event(
            "provider_request",
            json!({"provider": "minimax-music", "model": args.model, "dry_run": true}),
        );
        return Ok(());
    }
    let key =
        env::var("MINIMAX_API_KEY").map_err(|_| CliError::new(5, "MINIMAX_API_KEY is not set"))?;
    ctx.event(
        "provider_request",
        json!({"provider": "minimax-music", "model": args.model}),
    );
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
        write_json_atomic(&path, &body, true)?;
    }
    ctx.event(
        "artifact",
        json!({"path": args.out, "kind": "audio", "bytes": bytes.len()}),
    );
    Ok(())
}

fn audio_sfx(ctx: &Ctx, args: AudioSfxArgs) -> Result<()> {
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
                args.seed + i as u64,
                args.sample_rate,
                args.overwrite,
            )?;
            ctx.event("artifact", json!({"path": out, "kind": "audio"}));
        }
    }
    Ok(())
}

fn audio_trim(ctx: &Ctx, args: AudioTrimArgs) -> Result<()> {
    ensure_output(&args.out, args.overwrite)?;
    if args.end <= args.start {
        return Err(CliError::new(2, "--end must be greater than --start"));
    }
    let mut reader = hound::WavReader::open(&args.input)
        .map_err(|e| CliError::new(3, format!("open wav: {e}")))?;
    let spec = reader.spec();
    let channels = spec.channels as usize;
    let start = (args.start * spec.sample_rate as f32) as usize * channels;
    let end = (args.end * spec.sample_rate as f32) as usize * channels;
    let samples: Vec<i16> = reader
        .samples::<i16>()
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| CliError::new(3, format!("read wav samples: {e}")))?;
    let start = start.min(samples.len());
    let end = end.min(samples.len());
    let tmp = temp_path_for(&args.out);
    {
        let mut writer = hound::WavWriter::create(&tmp, spec)
            .map_err(|e| CliError::new(1, format!("create wav: {e}")))?;
        for s in &samples[start..end] {
            writer
                .write_sample(*s)
                .map_err(|e| CliError::new(1, e.to_string()))?;
        }
        writer
            .finalize()
            .map_err(|e| CliError::new(1, e.to_string()))?;
    }
    fs::rename(&tmp, &args.out)?;
    ctx.event("artifact", json!({"path": args.out, "kind": "audio"}));
    Ok(())
}

fn audio_waveform(ctx: &Ctx, args: AudioWaveformArgs) -> Result<()> {
    ensure_output(&args.out, args.overwrite)?;
    let mut reader = hound::WavReader::open(&args.input)
        .map_err(|e| CliError::new(3, format!("open wav: {e}")))?;
    let samples: Vec<i16> = reader
        .samples::<i16>()
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| CliError::new(3, format!("read wav samples: {e}")))?;
    let mut img = RgbaImage::from_pixel(args.width, args.height, Rgba([12, 14, 18, 255]));
    if !samples.is_empty() {
        for x in 0..args.width {
            let a = (x as usize * samples.len()) / args.width as usize;
            let b = (((x + 1) as usize * samples.len()) / args.width as usize)
                .max(a + 1)
                .min(samples.len());
            let peak = samples[a..b]
                .iter()
                .map(|s| (*s as f32).abs() / i16::MAX as f32)
                .fold(0.0, f32::max);
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
    let cols = args.cols.max(1);
    let rows = ((files.len() as u32) + cols - 1) / cols;
    let mut sheet =
        RgbaImage::from_pixel(cols * args.cell, rows * args.cell, Rgba([24, 26, 30, 255]));
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
    let codex = which::which(env::var("CODEX_BIN").unwrap_or_else(|_| "codex".into())).ok();
    let ffmpeg = which::which("ffmpeg").ok();
    let minimax = env::var("MINIMAX_API_KEY").is_ok();
    if ctx.json {
        ctx.event(
            "doctor",
            json!({
                "codex": codex.as_ref().map(|p| p.display().to_string()),
                "ffmpeg": ffmpeg.as_ref().map(|p| p.display().to_string()),
                "minimax_api_key": minimax
            }),
        );
    } else {
        println!(
            "codex: {}",
            codex
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "missing".into())
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
    }
    Ok(())
}

async fn run_codex_image(
    ctx: &Ctx,
    instruction: &str,
    refs: &[PathBuf],
    out: &Path,
    overwrite: bool,
    metadata_out: Option<&Path>,
    model: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    let codex_bin = env::var("CODEX_BIN").unwrap_or_else(|_| "codex".into());
    if which::which(&codex_bin).is_err() {
        return Err(CliError::new(5, format!("{codex_bin} not found in PATH")));
    }
    for r in refs {
        if !r.is_file() {
            return Err(CliError::new(
                3,
                format!("reference image not found: {}", r.display()),
            ));
        }
    }
    if dry_run {
        ctx.event(
            "provider_request",
            json!({"provider": "codex-image", "dry_run": true}),
        );
        return Ok(());
    }

    let tmpdir = TempDir::new().map_err(|e| CliError::new(1, e.to_string()))?;
    let rel_out = "asset.png";
    let final_instruction = format!(
        "{instruction}\n\nUse the native $imagegen tool directly. Write exactly one PNG file to `{rel_out}` in the current working directory. Create parent directories if needed. Do not write any project files, manifests, or extra state."
    );
    let mut cmd = Command::new(codex_bin);
    cmd.arg("exec")
        .arg("--ephemeral")
        .arg("--skip-git-repo-check")
        .arg("--sandbox")
        .arg(env::var("CODEX_SANDBOX").unwrap_or_else(|_| "workspace-write".into()))
        .arg("-C")
        .arg(tmpdir.path());
    if let Some(model) = model {
        cmd.arg("--model").arg(model);
    }
    cmd.arg(final_instruction);
    for r in refs {
        cmd.arg("--image").arg(r);
    }
    ctx.event("provider_request", json!({"provider": "codex-image"}));
    let output = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| CliError::new(6, format!("run codex: {e}")))?;
    if !output.status.success() {
        return Err(CliError::new(
            6,
            format!(
                "codex failed: {}",
                String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .last()
                    .unwrap_or("no stderr")
            ),
        ));
    }
    let generated = tmpdir.path().join(rel_out);
    if !generated.is_file() {
        return Err(CliError::new(
            6,
            "codex completed but did not create asset.png",
        ));
    }
    image::open(&generated)
        .map_err(|e| CliError::new(7, format!("generated file is not an image: {e}")))?;
    let bytes = fs::read(&generated)?;
    write_atomic(out, &bytes, overwrite)?;
    if let Some(meta) = metadata_out {
        write_json_atomic(
            meta,
            &json!({
                "provider": "codex-image",
                "stdout": String::from_utf8_lossy(&output.stdout),
                "stderr": String::from_utf8_lossy(&output.stderr),
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
) -> String {
    let mut s = String::new();
    s.push_str("Generate a production-ready 2D game asset.\n");
    s.push_str(&format!("Asset kind: {kind}.\n"));
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
    for i in 0..n {
        let t = i as f32 / sample_rate as f32;
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
    write_atomic_from_file(&tmp, out, overwrite)?;
    let _ = fs::remove_file(tmp);
    Ok(())
}

fn read_prompt(path: Option<&Path>, text: Option<&str>) -> Result<String> {
    match (path, text) {
        (Some(_), Some(_)) => Err(CliError::new(
            2,
            "use either --prompt or --prompt-text, not both",
        )),
        (Some(path), None) => fs::read_to_string(path)
            .map_err(|e| CliError::new(3, format!("{}: {e}", path.display()))),
        (None, Some(text)) => Ok(text.to_string()),
        (None, None) => Err(CliError::new(2, "prompt is required")),
    }
}

fn read_optional(path: Option<&Path>) -> Result<Option<String>> {
    path.map(fs::read_to_string)
        .transpose()
        .map_err(|e| CliError::new(3, e.to_string()))
}

fn ensure_output(path: &Path, overwrite: bool) -> Result<()> {
    if path.exists() && !overwrite {
        return Err(CliError::new(
            4,
            format!("output exists: {}", path.display()),
        ));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8], overwrite: bool) -> Result<()> {
    ensure_output(path, overwrite)?;
    let tmp = temp_path_for(path);
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)?;
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
    img.save(&tmp)
        .map_err(|e| CliError::new(1, format!("save image: {e}")))?;
    fs::rename(&tmp, path)?;
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
    Ok((
        w.parse().map_err(|_| CliError::new(2, "invalid width"))?,
        h.parse().map_err(|_| CliError::new(2, "invalid height"))?,
    ))
}

fn parse_box(s: &str) -> Result<(u32, u32, u32, u32)> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != 4 {
        return Err(CliError::new(2, "box must be x,y,w,h"));
    }
    Ok((
        parts[0]
            .parse()
            .map_err(|_| CliError::new(2, "invalid box x"))?,
        parts[1]
            .parse()
            .map_err(|_| CliError::new(2, "invalid box y"))?,
        parts[2]
            .parse()
            .map_err(|_| CliError::new(2, "invalid box w"))?,
        parts[3]
            .parse()
            .map_err(|_| CliError::new(2, "invalid box h"))?,
    ))
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

fn image_kind_name(kind: &ImageKind) -> &'static str {
    match kind {
        ImageKind::Scene => "scene",
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
    fn parses_size() {
        assert_eq!(parse_size("1280x720").unwrap(), (1280, 720));
        assert!(parse_size("1280,720").is_err());
    }

    #[test]
    fn parses_box() {
        assert_eq!(parse_box("1,2,3,4").unwrap(), (1, 2, 3, 4));
        assert!(parse_box("1,2,3").is_err());
    }

    #[test]
    fn parses_grid() {
        assert_eq!(parse_grid("8x2").unwrap(), (8, 2));
        assert!(parse_grid("0x2").is_err());
    }

    #[test]
    fn trims_alpha() {
        let mut img = RgbaImage::from_pixel(4, 4, Rgba([0, 0, 0, 0]));
        img.put_pixel(1, 2, Rgba([255, 255, 255, 255]));
        let out = trim_alpha(&img).unwrap();
        assert_eq!(out.dimensions(), (1, 1));
    }
}
