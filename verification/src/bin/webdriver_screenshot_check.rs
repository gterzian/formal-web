use clap::{Args, Parser, Subcommand};
use image::{ImageReader, RgbaImage};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(name = "webdriver-screenshot-check")]
#[command(about = "Inspect WebDriver screenshots for visual verification")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Visible(VisibleArgs),
    Diff(DiffArgs),
}

#[derive(Args, Debug)]
struct VisibleArgs {
    #[arg(long, value_name = "PNG")]
    png: PathBuf,

    #[command(flatten)]
    rect: RectArgs,

    #[arg(long, default_value_t = 12)]
    inset: u32,

    #[arg(long, default_value_t = 0.01)]
    min_non_white_ratio: f64,

    #[arg(long, default_value_t = 6.0)]
    min_luma_stddev: f64,

    #[arg(long, default_value_t = 8)]
    min_unique_buckets: usize,
}

#[derive(Args, Debug)]
struct DiffArgs {
    #[arg(long, value_name = "PNG")]
    before: PathBuf,

    #[arg(long, value_name = "PNG")]
    after: PathBuf,

    #[command(flatten)]
    rect: RectArgs,

    #[arg(long, default_value_t = 12)]
    inset: u32,

    #[arg(long, default_value_t = 12)]
    pixel_threshold: u8,

    #[arg(long, default_value_t = 0.01)]
    min_changed_ratio: f64,
}

#[derive(Args, Clone, Debug)]
struct RectArgs {
    #[arg(long)]
    x: u32,

    #[arg(long)]
    y: u32,

    #[arg(long)]
    width: u32,

    #[arg(long)]
    height: u32,
}

#[derive(Clone, Copy, Debug)]
struct RegionBounds {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
}

#[derive(Debug)]
struct VisibleMetrics {
    non_white_ratio: f64,
    luma_stddev: f64,
    unique_buckets: usize,
}

#[derive(Debug)]
struct DiffMetrics {
    changed_ratio: f64,
    max_channel_delta: u8,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("webdriver-screenshot-check: {error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let cli = Cli::parse();
    match cli.command {
        Command::Visible(args) => run_visible(args),
        Command::Diff(args) => run_diff(args),
    }
}

fn run_visible(args: VisibleArgs) -> Result<(), String> {
    let image = load_png(&args.png)?;
    let region = resolve_region(image.width(), image.height(), &args.rect, args.inset)?;
    let metrics = visible_metrics(&image, region);
    let has_visual_content = metrics.non_white_ratio >= args.min_non_white_ratio
        || metrics.luma_stddev >= args.min_luma_stddev
        || metrics.unique_buckets >= args.min_unique_buckets;

    if !has_visual_content {
        return Err(format!(
            "cropped region appears blank: non_white_ratio={:.4}, luma_stddev={:.2}, unique_buckets={} (thresholds: {:.4}, {:.2}, {})",
            metrics.non_white_ratio,
            metrics.luma_stddev,
            metrics.unique_buckets,
            args.min_non_white_ratio,
            args.min_luma_stddev,
            args.min_unique_buckets,
        ));
    }

    println!(
        "visible content confirmed: non_white_ratio={:.4}, luma_stddev={:.2}, unique_buckets={}",
        metrics.non_white_ratio, metrics.luma_stddev, metrics.unique_buckets,
    );
    Ok(())
}

fn run_diff(args: DiffArgs) -> Result<(), String> {
    let before = load_png(&args.before)?;
    let after = load_png(&args.after)?;

    if before.dimensions() != after.dimensions() {
        return Err(format!(
            "screenshots do not have matching dimensions: before={}x{}, after={}x{}",
            before.width(),
            before.height(),
            after.width(),
            after.height(),
        ));
    }

    let region = resolve_region(before.width(), before.height(), &args.rect, args.inset)?;
    let metrics = diff_metrics(&before, &after, region, args.pixel_threshold);
    if metrics.changed_ratio < args.min_changed_ratio {
        return Err(format!(
            "cropped region did not change enough: changed_ratio={:.4}, max_channel_delta={} (thresholds: {:.4}, {})",
            metrics.changed_ratio,
            metrics.max_channel_delta,
            args.min_changed_ratio,
            args.pixel_threshold,
        ));
    }

    println!(
        "region diff confirmed: changed_ratio={:.4}, max_channel_delta={}",
        metrics.changed_ratio, metrics.max_channel_delta,
    );
    Ok(())
}

fn load_png(path: &Path) -> Result<RgbaImage, String> {
    ImageReader::open(path)
        .map_err(|error| format!("failed to open {}: {error}", path.display()))?
        .decode()
        .map_err(|error| format!("failed to decode {}: {error}", path.display()))?
        .into_rgba8()
        .pipe(Ok)
}

fn resolve_region(
    image_width: u32,
    image_height: u32,
    rect: &RectArgs,
    inset: u32,
) -> Result<RegionBounds, String> {
    let x0 = rect.x.min(image_width);
    let y0 = rect.y.min(image_height);
    let x1 = rect.x.saturating_add(rect.width).min(image_width);
    let y1 = rect.y.saturating_add(rect.height).min(image_height);

    if x1 <= x0 || y1 <= y0 {
        return Err(format!(
            "requested region is outside the screenshot bounds: x={} y={} width={} height={} image={}x{}",
            rect.x, rect.y, rect.width, rect.height, image_width, image_height,
        ));
    }

    let inset_x = inset.min((x1 - x0).saturating_sub(1) / 2);
    let inset_y = inset.min((y1 - y0).saturating_sub(1) / 2);
    let bounds = RegionBounds {
        x0: x0 + inset_x,
        y0: y0 + inset_y,
        x1: x1 - inset_x,
        y1: y1 - inset_y,
    };

    if bounds.x1 <= bounds.x0 || bounds.y1 <= bounds.y0 {
        return Err(format!(
            "requested region collapsed after inset {}: x={} y={} width={} height={}",
            inset, rect.x, rect.y, rect.width, rect.height,
        ));
    }

    Ok(bounds)
}

fn visible_metrics(image: &RgbaImage, region: RegionBounds) -> VisibleMetrics {
    let mut total_pixels = 0_u64;
    let mut non_white_pixels = 0_u64;
    let mut luma_sum = 0.0_f64;
    let mut luma_sum_squares = 0.0_f64;
    let mut unique_buckets = HashSet::new();

    for y in region.y0..region.y1 {
        for x in region.x0..region.x1 {
            let [red, green, blue, _alpha] = image.get_pixel(x, y).0;
            if red < 245 || green < 245 || blue < 245 {
                non_white_pixels += 1;
            }

            let luma = 0.2126 * f64::from(red)
                + 0.7152 * f64::from(green)
                + 0.0722 * f64::from(blue);
            luma_sum += luma;
            luma_sum_squares += luma * luma;
            unique_buckets.insert(color_bucket(red, green, blue));
            total_pixels += 1;
        }
    }

    let total = total_pixels.max(1) as f64;
    let luma_mean = luma_sum / total;
    let luma_variance = (luma_sum_squares / total - luma_mean * luma_mean).max(0.0);
    VisibleMetrics {
        non_white_ratio: non_white_pixels as f64 / total,
        luma_stddev: luma_variance.sqrt(),
        unique_buckets: unique_buckets.len(),
    }
}

fn diff_metrics(
    before: &RgbaImage,
    after: &RgbaImage,
    region: RegionBounds,
    pixel_threshold: u8,
) -> DiffMetrics {
    let mut changed_pixels = 0_u64;
    let mut total_pixels = 0_u64;
    let mut max_channel_delta = 0_u8;

    for y in region.y0..region.y1 {
        for x in region.x0..region.x1 {
            let before_pixel = before.get_pixel(x, y).0;
            let after_pixel = after.get_pixel(x, y).0;
            let pixel_delta = before_pixel
                .into_iter()
                .zip(after_pixel)
                .map(|(before_channel, after_channel)| before_channel.abs_diff(after_channel))
                .max()
                .unwrap_or(0);
            max_channel_delta = max_channel_delta.max(pixel_delta);
            if pixel_delta >= pixel_threshold {
                changed_pixels += 1;
            }
            total_pixels += 1;
        }
    }

    let total = total_pixels.max(1) as f64;
    DiffMetrics {
        changed_ratio: changed_pixels as f64 / total,
        max_channel_delta,
    }
}

fn color_bucket(red: u8, green: u8, blue: u8) -> u32 {
    (u32::from(red / 32) << 16) | (u32::from(green / 32) << 8) | u32::from(blue / 32)
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}

impl<T> Pipe for T {}