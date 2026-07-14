fn main() {
    let (input, output) = match audio_analyzer::parse_args(std::env::args()) {
        Ok(arguments) => arguments,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    let report = audio_analyzer::analyze_file(&input);
    let mut json = match serde_json::to_string_pretty(&report) {
        Ok(json) => json,
        Err(error) => {
            eprintln!("Analysis report serialization failed: {error}");
            std::process::exit(2);
        }
    };
    json.push('\n');
    if let Some(path) = output {
        if let Err(error) = audio_analyzer::write_report_noclobber(&path, json.as_bytes()) {
            eprintln!(
                "Analysis report was not written to {} (existing files are never overwritten): {error}",
                path.display()
            );
            std::process::exit(2);
        }
    } else {
        print!("{json}");
    }
    if report.has_hard_rejections() {
        std::process::exit(1);
    }
}
