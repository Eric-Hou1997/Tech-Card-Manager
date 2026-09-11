fn main() {
    let output = tcm_core::typescript();
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--check") {
        let matches = args
            .get(1)
            .and_then(|p| std::fs::read_to_string(p).ok())
            .is_some_and(|s| s == output);
        if !matches {
            eprintln!("Generated TypeScript contracts differ; regenerate before building");
            std::process::exit(1);
        }
    } else if args.is_empty() {
        print!("{output}");
    } else {
        eprintln!("Usage: export_types [--check path]");
        std::process::exit(2);
    }
}
