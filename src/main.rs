use std::process;

fn main() {
    let result = codex_ops::run_cli(std::env::args().skip(1));

    print!("{}", result.stdout);
    eprint!("{}", result.stderr);
    process::exit(result.code);
}
