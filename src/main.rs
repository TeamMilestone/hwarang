use std::path::PathBuf;
use std::process;

use clap::Parser;

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

fn main() {
    let args = Args::parse();

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

    match hwp_text_extract::extract_text_from_file(&args.input) {
        Ok(text) => print!("{}", text),
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    }
}
