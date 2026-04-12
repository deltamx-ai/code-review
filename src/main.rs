fn main() {
    if let Err(err) = code_review::run() {
        eprintln!("Error: {err:#}");
        std::process::exit(1);
    }
}
