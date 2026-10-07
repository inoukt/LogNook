#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--agent-mcp")) {
        if let Err(error) = lognook_desktop_lib::run_agent_mcp() {
            eprintln!("{error}");
            std::process::exit(1);
        }
    } else {
        lognook_desktop_lib::run();
    }
}
