//! Benchmark for the representative runtime workload in `RUN-005`.
//!
//! The target is p95 below 100 ms on a typical local SSD for a warm-cache 1 MiB
//! textual payload, 100 resolved values, and 10 dotenv files. This is an
//! engineering benchmark, not a machine-independent guarantee, so it reports
//! measurements and never fails on timing. CI must stay timing-independent.
//!
//! Run it with `mise run bench`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use contextveil::registry::{self, Outcome};
use contextveil::source::Environment;

const DOTENV_FILES: usize = 10;
const KEYS_PER_FILE: usize = 10;
const PAYLOAD_BYTES: usize = 1024 * 1024;
const ITERATIONS: usize = 60;
const TARGET: Duration = Duration::from_millis(100);

fn main() {
    let fixture = Fixture::new();
    let environment = fixture.environment();
    let project_root = fixture.project_root();
    let payload = fixture.payload();

    // Warm the cache the requirement assumes, and check the workload is real.
    let active = match registry::build(&environment, Some(&project_root)) {
        Outcome::Ready(registry) => registry.redactor.active_count(),
        Outcome::Malfunction(malfunction) => panic!("benchmark setup failed: {malfunction:?}"),
    };
    assert_eq!(active, DOTENV_FILES * KEYS_PER_FILE);

    let mut durations: Vec<Duration> = Vec::with_capacity(ITERATIONS);
    let mut replacements = 0;
    for _ in 0..ITERATIONS {
        let started = Instant::now();
        // One complete event: load both config files, resolve every source
        // afresh (`SRC-009`), and redact the payload.
        let outcome = registry::build(&environment, Some(&project_root));
        let Outcome::Ready(effective) = outcome else {
            panic!("benchmark run failed");
        };
        let mut tally = effective.redactor.tally();
        let redacted = effective.redactor.redact(&payload, &mut tally);
        durations.push(started.elapsed());
        replacements = tally.total();
        std::hint::black_box(redacted);
    }

    durations.sort();
    let percentile = |fraction: f64| {
        let index = ((durations.len() as f64 * fraction).ceil() as usize).saturating_sub(1);
        durations[index.min(durations.len() - 1)]
    };
    let p50 = percentile(0.50);
    let p95 = percentile(0.95);

    println!("ContextVeil redaction benchmark (RUN-005)");
    println!("  payload         {} KiB", PAYLOAD_BYTES / 1024);
    println!("  active values   {active}");
    println!("  dotenv files    {DOTENV_FILES}");
    println!("  replacements    {replacements} per run");
    println!("  iterations      {ITERATIONS}");
    println!("  p50             {:.1} ms", p50.as_secs_f64() * 1000.0);
    println!("  p95             {:.1} ms", p95.as_secs_f64() * 1000.0);
    println!("  target          p95 < {} ms", TARGET.as_millis());
    println!(
        "  result          {}",
        if p95 < TARGET {
            "within target"
        } else {
            "ABOVE TARGET on this machine"
        }
    );
}

/// An isolated global config, project config, and dotenv corpus.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("contextveil-bench-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("config").join("contextveil")).expect("config directory");
        std::fs::create_dir_all(root.join("project")).expect("project directory");

        let mut project_config = String::from("version = 1\n");
        for file in 0..DOTENV_FILES {
            let name = format!(".env.{file}");
            let mut contents = String::new();
            for key in 0..KEYS_PER_FILE {
                contents.push_str(&format!(
                    "SSBENCH_KEY_{file}_{key}=bench-value-{file}-{key}\n"
                ));
            }
            std::fs::write(root.join("project").join(&name), contents).expect("write dotenv");
            project_config.push_str(&format!(
                "\n[[secret]]\nsource = \"dotenv\"\nfile = \"{name}\"\nall = true\n"
            ));
        }
        std::fs::write(
            root.join("project").join(".contextveil.toml"),
            project_config,
        )
        .expect("write project config");
        std::fs::write(
            root.join("config").join("contextveil").join("config.toml"),
            "version = 1\n",
        )
        .expect("write global config");

        Self { root }
    }

    fn environment(&self) -> Environment {
        Environment::from_pairs([
            (
                "XDG_CONFIG_HOME",
                self.root.join("config").to_string_lossy().into_owned(),
            ),
            ("HOME", self.root.to_string_lossy().into_owned()),
        ])
    }

    fn project_root(&self) -> PathBuf {
        self.root.join("project")
    }

    /// A 1 MiB payload with a realistic sprinkling of matches.
    fn payload(&self) -> String {
        let filler = "log line with ordinary tool output and no enrolled value at all\n";
        let mut payload = String::with_capacity(PAYLOAD_BYTES + 128);
        let mut index = 0;
        while payload.len() < PAYLOAD_BYTES {
            payload.push_str(filler);
            if index % 40 == 0 {
                let file = (index / 40) % DOTENV_FILES;
                let key = (index / 40) % KEYS_PER_FILE;
                payload.push_str(&format!("TOKEN=bench-value-{file}-{key}\n"));
            }
            index += 1;
        }
        payload
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
