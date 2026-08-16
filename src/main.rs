use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let mut stdout = std::io::stdout();
    let mut stderr = std::io::stderr();
    let code = secretsieve::cli::run(&args, &mut stdout, &mut stderr);
    let _ = stdout.flush();
    let _ = stderr.flush();
    ExitCode::from(code.as_u8())
}
