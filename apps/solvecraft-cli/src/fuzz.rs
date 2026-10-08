//! `solvecraft-cli fuzz`: random modelling sessions and boolean checks (docs/robustness.md).
//!
//! Each seed runs in its own process (`fuzz-one`) with a time limit, so a hang or a hard crash
//! is recorded like any other failure.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use solvecraft_engine::fuzz;
use solvecraft_engine::fuzz::boolean_case;

fn flag<T: std::str::FromStr>(args: &[String], name: &str, default: T) -> T {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// One seed: a design and a boolean case, as JSON on stdout (progress on stderr).
pub fn one(args: &[String]) -> Result<(), String> {
    let seed: u64 = args.first().and_then(|s| s.parse().ok()).ok_or("fuzz-one <seed> [--steps N] [--minimise]")?;
    let steps = flag(args, "--steps", 8usize);
    let minimise = args.iter().any(|a| a == "--minimise");
    eprintln!("design {seed}");
    let run = fuzz::run_with(seed, steps, &mut |st| eprintln!("step {}", serde_json::to_string(st).unwrap_or_default()));
    let mut design = json!({
        "outcome": run.outcome,
        "steps": run.steps.iter().map(|s| s.kind).collect::<Vec<_>>(),
        "unsupported": run.unsupported,
        "rejected": run.rejected,
    });
    if run.outcome.is_bug() {
        design["script"] = run.script();
        if minimise {
            eprintln!("minimise");
            let m = fuzz::minimise(&run);
            design["minimal"] = json!({"outcome": m.outcome, "steps": m.steps.iter().map(|s| s.kind).collect::<Vec<_>>(), "script": m.script()});
        }
    }
    let (a, b) = solvecraft_engine::fuzz::boolean_shapes(seed);
    eprintln!("boolean {seed} {} {}", serde_json::to_string(&a).unwrap_or_default(), serde_json::to_string(&b).unwrap_or_default());
    let b = boolean_case(seed);
    println!("{}", json!({"seed": seed, "design": design, "boolean": b}));
    Ok(())
}

struct Job {
    seed: u64,
    child: std::process::Child,
    started: Instant,
    err: std::sync::mpsc::Receiver<String>,
    last: String,
}

/// Run seeds `--from`…`--from + --count` in `--jobs` processes, `--timeout` seconds each;
/// write failing scripts to `--out` and a summary to `--report`.
pub fn run(args: &[String]) -> Result<(), String> {
    let from = flag(args, "--from", 0u64);
    let count = flag(args, "--count", 100u64);
    let jobs = flag(args, "--jobs", 4usize).clamp(1, 64);
    let timeout = Duration::from_secs(flag(args, "--timeout", 120u64));
    let steps = flag(args, "--steps", 8usize);
    let out_dir = args.iter().position(|a| a == "--out").and_then(|i| args.get(i + 1)).cloned();
    let report = args.iter().position(|a| a == "--report").and_then(|i| args.get(i + 1)).cloned();
    let minimise = args.iter().any(|a| a == "--minimise");
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if let Some(d) = &out_dir {
        std::fs::create_dir_all(d).map_err(|e| format!("{d}: {e}"))?;
    }
    let mut next = from;
    let mut running: Vec<Job> = Vec::new();
    let mut results: Vec<Value> = Vec::new();
    let t0 = Instant::now();
    while next < from + count || !running.is_empty() {
        while running.len() < jobs && next < from + count {
            let mut cmd = Command::new(&exe);
            cmd.args(["fuzz-one", &next.to_string(), "--steps", &steps.to_string()]);
            if minimise {
                cmd.arg("--minimise");
            }
            let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
            let (tx, rx) = std::sync::mpsc::channel();
            if let Some(err) = child.stderr.take() {
                std::thread::spawn(move || {
                    for line in BufReader::new(err).lines().map_while(Result::ok) {
                        if tx.send(line).is_err() {
                            break;
                        }
                    }
                });
            }
            running.push(Job { seed: next, child, started: Instant::now(), err: rx, last: String::new() });
            next += 1;
        }
        std::thread::sleep(Duration::from_millis(50));
        let mut i = 0;
        while i < running.len() {
            let Some(j) = running.get_mut(i) else { break };
            while let Ok(l) = j.err.try_recv() {
                if l.starts_with("step") || l.starts_with("boolean") || l.starts_with("minimise") {
                    j.last = l;
                }
            }
            let done = match j.child.try_wait() {
                Ok(Some(status)) => {
                    let mut s = String::new();
                    if let Some(mut o) = j.child.stdout.take() {
                        let _ = o.read_to_string(&mut s);
                    }
                    Some(match serde_json::from_str::<Value>(s.trim()) {
                        Ok(v) => v,
                        Err(_) => json!({"seed": j.seed, "crash": format!("exit {status}"), "at": j.last}),
                    })
                }
                Ok(None) if j.started.elapsed() > timeout => {
                    let _ = j.child.kill();
                    let _ = j.child.wait();
                    Some(json!({"seed": j.seed, "timeout": timeout.as_secs(), "at": j.last}))
                }
                Ok(None) => None,
                Err(e) => Some(json!({"seed": j.seed, "crash": e.to_string(), "at": j.last})),
            };
            match done {
                Some(v) => {
                    record(&v, out_dir.as_deref());
                    results.push(v);
                    running.swap_remove(i);
                }
                None => i += 1,
            }
        }
    }
    let summary = summarise(&results, t0.elapsed());
    println!("{summary}");
    if let Some(r) = report {
        std::fs::write(&r, &summary).map_err(|e| format!("{r}: {e}"))?;
    }
    Ok(())
}

/// Print a failure as it comes in and keep its script.
fn record(v: &Value, out_dir: Option<&str>) {
    let seed = v["seed"].as_u64().unwrap_or(0);
    let d = &v["design"];
    let bug = matches!(d["outcome"]["kind"].as_str(), Some("failed" | "panic" | "invalid" | "volume"));
    if bug || v.get("timeout").is_some() || v.get("crash").is_some() {
        eprintln!("seed {seed}: {}", if bug { format!("{} {}", d["outcome"]["kind"], d["outcome"]["detail"]) } else { format!("{v}") });
    }
    let bk = v["boolean"]["outcome"].as_str().unwrap_or("ok");
    if bk != "ok" {
        eprintln!("seed {seed}: boolean {bk} {}", v["boolean"]["detail"]);
    }
    if let Some(dir) = out_dir {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(format!("{dir}/results.jsonl")) {
            let _ = writeln!(f, "{v}");
        }
        if bug {
            let script = if d.get("minimal").is_some() { &d["minimal"]["script"] } else { &d["script"] };
            let _ = std::fs::write(format!("{dir}/design-{seed}.json"), serde_json::to_string_pretty(script).unwrap_or_default());
        }
        if bk != "ok" {
            let _ = std::fs::write(format!("{dir}/boolean-{seed}.json"), serde_json::to_string_pretty(&v["boolean"]).unwrap_or_default());
        }
    }
}

fn summarise(results: &[Value], took: Duration) -> String {
    let n = results.len().max(1);
    let mut design: BTreeMap<String, usize> = BTreeMap::new();
    let mut boolean: BTreeMap<String, usize> = BTreeMap::new();
    let mut at_step: BTreeMap<String, usize> = BTreeMap::new();
    let mut unsupported: BTreeMap<String, usize> = BTreeMap::new();
    let mut steps = 0;
    for v in results {
        let k = if v.get("timeout").is_some() {
            "timeout".to_string()
        } else if v.get("crash").is_some() {
            "crash".to_string()
        } else {
            v["design"]["outcome"]["kind"].as_str().unwrap_or("?").to_string()
        };
        if k != "ok" {
            let last = v["design"]["steps"].as_array().and_then(|a| a.last()).and_then(Value::as_str).unwrap_or("?");
            *at_step.entry(format!("{k} at {last}")).or_default() += 1;
        }
        *design.entry(k).or_default() += 1;
        *boolean.entry(v["boolean"]["outcome"].as_str().unwrap_or("not run").to_string()).or_default() += 1;
        steps += v["design"]["steps"].as_array().map(Vec::len).unwrap_or(0);
        for u in v["design"]["unsupported"].as_array().into_iter().flatten() {
            *unsupported.entry(u[0].as_str().unwrap_or("?").to_string()).or_default() += 1;
        }
    }
    let pct = |c: usize| format!("{c} ({:.1} %)", 100.0 * c as f64 / n as f64);
    let mut s = format!("{} seeds, {steps} design steps, {:.0} s\n\nDesigns:\n", results.len(), took.as_secs_f64());
    for (k, c) in &design {
        s += &format!("- {k}: {}\n", pct(*c));
    }
    s += "\nFailures by step:\n";
    for (k, c) in &at_step {
        s += &format!("- {k}: {c}\n");
    }
    s += "\nSteps refused as not supported (undone, the design went on):\n";
    for (k, c) in &unsupported {
        s += &format!("- {k}: {c}\n");
    }
    s += "\nBoolean identity (union, intersection, cut of two primitives):\n";
    for (k, c) in &boolean {
        s += &format!("- {k}: {}\n", pct(*c));
    }
    s
}
