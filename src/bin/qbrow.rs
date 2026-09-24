//! The `qbrow` command: a real web browser inside the terminal.

fn main() -> std::io::Result<std::process::ExitCode> {
    qbrowser::run()
}
