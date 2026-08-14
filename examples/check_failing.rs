fn main() {
    for p in std::env::args().skip(1) {
        let path = std::path::Path::new(&p);
        let name = path.file_name().unwrap().to_string_lossy();
        match hwarang::extract_text_from_file(path) {
            Ok(t) => println!("  {name:32} OK ({} chars)", t.chars().count()),
            Err(e) => println!("  {name:32} ERR: {e}"),
        }
    }
}
