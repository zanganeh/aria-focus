fn main() {
    match candidate_ledger::run(std::env::args_os()) {
        Ok(()) => {}
        Err(error) => {
            eprintln!("candidate-ledger: {error}");
            std::process::exit(2);
        }
    }
}
