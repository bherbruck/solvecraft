//! solidcraft-cam binary entry point.
//!
//! On startup prints **exactly one line** to stdout:
//! ```text
//! solidcraft-cam token=<TOKEN> address=http://127.0.0.1:<PORT>
//! ```

use std::env;
use std::path::PathBuf;

use solidcraft_cam::{
    auth::generate_token,
    config::{CamConfig, DEFAULT_JOB_TIMEOUT_SECS},
    origin::ParsedOrigin,
    run,
};

#[tokio::main]
async fn main() {
    let cfg = match parse_args() {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("solidcraft-cam: {msg}");
            eprintln!("Run with --help for usage.");
            std::process::exit(1);
        }
    };

    if let Err(e) = run(cfg).await {
        eprintln!("solidcraft-cam: fatal: {e}");
        std::process::exit(1);
    }
}

fn parse_args() -> Result<CamConfig, String> {
    let mut args = env::args().skip(1);
    let mut origins: Vec<ParsedOrigin> = Vec::new();
    let mut temp_dir: Option<PathBuf> = None;
    let mut freecad_exec = "freecadcmd".to_string();
    let mut adapter_path: Option<PathBuf> = None;
    let mut timeout = DEFAULT_JOB_TIMEOUT_SECS;
    let mut token: Option<String> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            "--port" => {
                // Accepted but ignored: we always bind to OS-assigned port on loopback.
                let _ = args.next().ok_or("--port requires a value")?;
            }
            "--origin" => {
                let v = args.next().ok_or("--origin requires a value")?;
                let parsed = ParsedOrigin::parse(&v).map_err(|e| format!("invalid origin `{v}`: {e}"))?;
                origins.push(parsed);
            }
            "--temp-dir" => {
                let v = args.next().ok_or("--temp-dir requires a value")?;
                temp_dir = Some(PathBuf::from(v));
            }
            "--freecad-exec" => {
                freecad_exec = args.next().ok_or("--freecad-exec requires a value")?;
            }
            "--adapter" => {
                let v = args.next().ok_or("--adapter requires a value")?;
                adapter_path = Some(PathBuf::from(v));
            }
            "--timeout" => {
                let v = args.next().ok_or("--timeout requires a value")?;
                timeout = v.parse::<u64>().map_err(|_| format!("invalid timeout: {v}"))?;
            }
            "--token" => {
                token = Some(args.next().ok_or("--token requires a value")?);
            }
            other => {
                return Err(format!("unknown argument: {other}"));
            }
        }
    }

    let temp_base = temp_dir.unwrap_or_else(std::env::temp_dir);

    let adapter = match adapter_path {
        Some(p) => p,
        None => {
            let exe = env::current_exe().map_err(|e| format!("cannot find executable path: {e}"))?;
            exe.parent().ok_or("executable has no parent directory")?.join("adapter").join("freecad_cam_adapter.py")
        }
    };

    Ok(CamConfig {
        token: token.unwrap_or_else(generate_token),
        allowed_origins: origins,
        temp_base,
        freecad_exec,
        adapter_path: adapter,
        job_timeout_secs: timeout,
    })
}

fn print_usage() {
    eprintln!(concat!(
        "solidcraft-cam — FreeCAD CAM loopback HTTP companion\n",
        "\n",
        "USAGE:\n",
        "  solidcraft-cam [OPTIONS]\n",
        "\n",
        "OPTIONS:\n",
        "  --port <PORT>          Port to listen on (ignored; always OS-assigned)\n",
        "  --origin <ORIGIN>      Allowed CORS origin, repeatable\n",
        "  --temp-dir <PATH>      Base dir for per-job temp workspaces\n",
        "  --freecad-exec <PATH>  FreeCAD Python executable (default: freecadcmd)\n",
        "  --adapter <PATH>       Path to freecad_cam_adapter.py\n",
        "  --timeout <SECS>       Per-job subprocess timeout (default: 300)\n",
        "  --token <TOKEN>        Fixed bearer token (testing only)\n",
        "  --help                 Print help\n",
        "\n",
        "Startup line printed to stdout:\n",
        "  solidcraft-cam token=<TOKEN> address=http://127.0.0.1:<PORT>\n",
    ));
}
