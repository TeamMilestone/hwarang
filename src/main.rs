use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use clap::Parser;
use rayon::prelude::*;

#[derive(Parser, Debug)]
#[command(name = "hwp-text-extract", about = "HWP 문서 텍스트 추출기")]
struct Args {
    /// 입력 HWP 파일 또는 디렉토리
    input: PathBuf,

    /// 출력 디렉토리 (지정 시 파일별 .txt 생성)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// 병렬 처리 스레드 수 (기본: CPU 코어 수)
    #[arg(short = 'j', long)]
    threads: Option<usize>,

    /// 하위 디렉토리 재귀 탐색
    #[arg(short, long)]
    recursive: bool,

    /// 스트림 목록만 출력
    #[arg(long)]
    list_streams: bool,
}

fn collect_hwp_files(dir: &Path, recursive: bool) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Error reading directory {:?}: {}", dir, e);
            return files;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if recursive {
                files.extend(collect_hwp_files(&path, true));
            }
        } else if path
            .extension()
            .map_or(false, |ext| ext.eq_ignore_ascii_case("hwp"))
        {
            files.push(path);
        }
    }
    files
}

fn process_batch(files: &[PathBuf], output_dir: &Path) {
    let start = Instant::now();
    let total = files.len();
    let success = AtomicUsize::new(0);
    let failed = AtomicUsize::new(0);

    files.par_iter().for_each(|path| {
        match hwp_text_extract::extract_text_from_file(path) {
            Ok(text) => {
                let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                let out_path = output_dir.join(format!("{}.txt", stem));
                if let Err(e) = fs::write(&out_path, &text) {
                    eprintln!("WRITE_ERR\t{}\t{}", path.display(), e);
                    failed.fetch_add(1, Ordering::Relaxed);
                } else {
                    success.fetch_add(1, Ordering::Relaxed);
                }
            }
            Err(e) => {
                eprintln!("EXTRACT_ERR\t{}\t{}", path.display(), e);
                failed.fetch_add(1, Ordering::Relaxed);
            }
        }
    });

    let elapsed = start.elapsed();
    let ok = success.load(Ordering::Relaxed);
    let fail = failed.load(Ordering::Relaxed);
    eprintln!(
        "Done: {}/{} succeeded, {} failed, {:.2}s ({:.0} files/s)",
        ok,
        total,
        fail,
        elapsed.as_secs_f64(),
        total as f64 / elapsed.as_secs_f64()
    );
}

fn process_batch_with_structure(files: &[PathBuf], base_dir: &Path, output_dir: &Path) {
    let start = Instant::now();
    let total = files.len();
    let success = AtomicUsize::new(0);
    let failed = AtomicUsize::new(0);

    files.par_iter().for_each(|path| {
        match hwp_text_extract::extract_text_from_file(path) {
            Ok(text) => {
                // 입력 디렉토리 기준 상대 경로 유지
                let rel = path.strip_prefix(base_dir).unwrap_or(path);
                let mut out_path = output_dir.join(rel);
                out_path.set_extension("txt");

                if let Some(parent) = out_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }

                if let Err(e) = fs::write(&out_path, &text) {
                    eprintln!("WRITE_ERR\t{}\t{}", path.display(), e);
                    failed.fetch_add(1, Ordering::Relaxed);
                } else {
                    success.fetch_add(1, Ordering::Relaxed);
                }
            }
            Err(e) => {
                eprintln!("EXTRACT_ERR\t{}\t{}", path.display(), e);
                failed.fetch_add(1, Ordering::Relaxed);
            }
        }
    });

    let elapsed = start.elapsed();
    let ok = success.load(Ordering::Relaxed);
    let fail = failed.load(Ordering::Relaxed);
    eprintln!(
        "Done: {}/{} succeeded, {} failed, {:.2}s ({:.0} files/s)",
        ok,
        total,
        fail,
        elapsed.as_secs_f64(),
        total as f64 / elapsed.as_secs_f64()
    );
}

fn main() {
    let args = Args::parse();

    // rayon 스레드풀 설정 (4MB 스택 사이즈: 깊은 중첩 문서 대비)
    {
        let mut builder = rayon::ThreadPoolBuilder::new().stack_size(4 * 1024 * 1024);
        if let Some(n) = args.threads {
            builder = builder.num_threads(n);
        }
        builder.build_global().unwrap();
    }

    if args.list_streams {
        match hwp_text_extract::list_streams(&args.input) {
            Ok(streams) => {
                for s in &streams {
                    println!("{}", s);
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                process::exit(1);
            }
        }
        return;
    }

    // 단일 파일 모드
    if args.input.is_file() {
        if let Some(ref out_dir) = args.output {
            fs::create_dir_all(out_dir).unwrap_or_else(|e| {
                eprintln!("Error creating output directory: {}", e);
                process::exit(1);
            });
            process_batch(&[args.input.clone()], out_dir);
        } else {
            match hwp_text_extract::extract_text_from_file(&args.input) {
                Ok(text) => print!("{}", text),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    process::exit(1);
                }
            }
        }
        return;
    }

    // 디렉토리 모드: 반드시 -o 필요
    if !args.input.is_dir() {
        eprintln!("Error: {:?} is not a file or directory", args.input);
        process::exit(1);
    }

    let output_dir = match args.output {
        Some(ref d) => d.clone(),
        None => {
            eprintln!("Error: output directory (-o) required for directory input");
            process::exit(1);
        }
    };

    fs::create_dir_all(&output_dir).unwrap_or_else(|e| {
        eprintln!("Error creating output directory: {}", e);
        process::exit(1);
    });

    let files = collect_hwp_files(&args.input, args.recursive);
    eprintln!("Found {} HWP files", files.len());

    if files.is_empty() {
        return;
    }

    if args.recursive {
        process_batch_with_structure(&files, &args.input, &output_dir);
    } else {
        process_batch(&files, &output_dir);
    }
}
