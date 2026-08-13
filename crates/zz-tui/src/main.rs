use std::process::ExitCode;

fn main() -> ExitCode {
    match zz_tui::run_cli(std::env::args().skip(1)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("zz-tui: {error}");
            ExitCode::FAILURE
        }
    }
}
