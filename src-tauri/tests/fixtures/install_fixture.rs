//! Test-only ADB substitute: records argv, never launches another command.
use std::{env, fs::OpenOptions, io::Write, process, time::Duration};
fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() < 3 || args[0] != "-s" { process::exit(24); }
    let scenario = &args[1];
    let trace = env::current_exe().unwrap().with_extension(format!("{scenario}.trace"));
    let mut log = OpenOptions::new().create(true).append(true).open(trace).unwrap();
    writeln!(log, "{}", args.join("|")).unwrap();
    if args[2] == "shell" {
        if scenario == "missing" { return; }
        if scenario == "read-error" { process::exit(9); }
        let source = args[3..].join(" ");
        if source != "'pm' 'path' 'com.example.review'" { process::exit(25); }
        println!("package:/data/app/com.example.review/base.apk");
        return;
    }
    if args[2] != "install" { process::exit(26); }
    match scenario.as_str() {
        "exit-error" => { println!("Success"); process::exit(23); }
        "substring" => println!("unsuccessful"),
        "device-failure" => { println!("Success"); eprintln!("Failure [INSTALL_FAILED_INVALID_APK]"); }
        "fallback" if args.iter().any(|a| a == "--streaming") => {
            eprintln!("unknown option --streaming"); process::exit(1);
        }
        "cancel" => {
            println!("INSTALL_STARTED"); std::io::stdout().flush().unwrap();
            std::thread::sleep(Duration::from_secs(5));
        }
        _ => println!("Success"),
    }
}
