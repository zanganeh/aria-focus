fn main() {
    let (source, manifest, output) = match content_ingest::parse_args(std::env::args()) {
        Ok(values) => values,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    if let Err(error) = content_ingest::build_pack(&source, &manifest, &output) {
        eprintln!("Content pack was not created: {error}");
        std::process::exit(1);
    }
    println!("Created validated content pack: {}", output.display());
}
