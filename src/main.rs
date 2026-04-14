fn main() {
    match code_review::run() {
        Ok(code) => std::process::exit(code),
        Err(err) => {
            eprintln!("Error: {err:#}");
            std::process::exit(5);
        }
    }
}
