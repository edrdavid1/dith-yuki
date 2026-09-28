//! `dither-shell-diag` — same extraction code as the shell DLL.

use dither_shell::{
    check_lines, clear_cache, composite_centered, recent_lines, register, render_file, render_text,
    shell_thumb_rgba, unpremultiply_rgba, unregister, Hive,
};
use std::env;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(cmd) = args.next() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    match cmd.as_str() {
        "check" => cmd_check(&parse_hive(args.next().as_deref())),
        "render" => cmd_render(args),
        "shell-thumb" => cmd_shell(args),
        "register" => cmd_reg(true, args),
        "unregister" => cmd_reg(false, args),
        "clear-cache" => match clear_cache() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::from(1)
            }
        },
        "preview-host" => cmd_preview_host(args),
        "last-errors" => {
            println!("{}", recent_lines());
            ExitCode::SUCCESS
        }
        "help" | "-h" | "--help" => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("unknown command {other}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

const USAGE: &str = "\
dither-shell-diag check [--user|--machine]
dither-shell-diag render <file> --size N --out out.png
dither-shell-diag shell-thumb <file> --size N --out out.png
dither-shell-diag register|unregister [--user|--machine] [--dll path]
dither-shell-diag clear-cache
dither-shell-diag preview-host <file> --size N --out out.png
dither-shell-diag last-errors";

fn parse_hive(flag: Option<&str>) -> Hive {
    match flag {
        Some("--machine") => Hive::Machine,
        _ => Hive::User,
    }
}

fn cmd_check(hive: &Hive) -> ExitCode {
    let lines = check_lines(*hive);
    let mut fail = false;
    for line in &lines {
        println!("{line}");
        if line.contains(" FAIL") {
            fail = true;
        }
    }
    if fail {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn cmd_preview_host(args: impl Iterator<Item = String>) -> ExitCode {
    match parse_file_op(args) {
        Ok((file, size, out)) => match render_file(Path::new(&file), size.max(1)) {
            Ok((report, px)) => {
                let canvas = composite_centered(
                    report.width,
                    report.height,
                    &px,
                    size.max(1),
                    size.max(1),
                    [255, 255, 255, 255],
                );
                if let Err(e) = write_png(Path::new(&out), size.max(1), size.max(1), &canvas) {
                    eprintln!("write png: {e}");
                    return ExitCode::from(1);
                }
                println!(
                    "preview pane {}x{} bitmap {}x{}",
                    size, size, report.width, report.height
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::from(1)
            }
        },
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}

fn cmd_render(args: impl Iterator<Item = String>) -> ExitCode {
    match parse_file_op(args) {
        Ok((file, size, out)) => match render_file(Path::new(&file), size) {
            Ok((report, px)) => {
                if let Err(e) = write_png(Path::new(&out), report.width, report.height, &px) {
                    eprintln!("write png: {e}");
                    return ExitCode::from(1);
                }
                print!("{}", render_text(&report));
                if report.opaque_bbox_full {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(1)
                }
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::from(1)
            }
        },
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}

fn cmd_shell(args: impl Iterator<Item = String>) -> ExitCode {
    match parse_file_op(args) {
        Ok((file, size, out)) => match shell_thumb_rgba(Path::new(&file), size) {
            Ok((w, h, px)) => {
                if let Err(e) = write_png(Path::new(&out), w, h, &px) {
                    eprintln!("write png: {e}");
                    return ExitCode::from(1);
                }
                println!("size {w}x{h}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::from(1)
            }
        },
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}

fn cmd_reg(install: bool, mut args: impl Iterator<Item = String>) -> ExitCode {
    let mut hive = Hive::User;
    let mut dll = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--machine" => hive = Hive::Machine,
            "--user" => hive = Hive::User,
            "--dll" => dll = args.next(),
            other => {
                eprintln!("unknown flag {other}");
                return ExitCode::from(2);
            }
        }
    }
    let result = if install {
        register(hive, dll.as_deref())
    } else {
        unregister(hive)
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}

fn parse_file_op(args: impl Iterator<Item = String>) -> Result<(String, u32, String), String> {
    let args: Vec<String> = args.collect();
    let mut file = None;
    let mut size = None;
    let mut out = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--size" => {
                i += 1;
                size = args.get(i).and_then(|s| s.parse().ok());
            }
            "--out" => {
                i += 1;
                out = args.get(i).cloned();
            }
            other if !other.starts_with('-') && file.is_none() => file = Some(other.to_string()),
            other => return Err(format!("unexpected {other}")),
        }
        i += 1;
    }
    Ok((
        file.ok_or("missing file")?,
        size.ok_or("missing --size")?,
        out.ok_or("missing --out")?,
    ))
}

fn write_png(path: &Path, width: u32, height: u32, premul_rgba: &[u8]) -> std::io::Result<()> {
    let straight = unpremultiply_rgba(premul_rgba);
    let file = File::create(path)?;
    let mut enc = png::Encoder::new(BufWriter::new(file), width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .map_err(|e| std::io::Error::other(e.to_string()))?
        .write_image_data(&straight)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(())
}
