use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use contextveil::testing::Canary;

pub struct ProcessFixture {
    root: PathBuf,
}

impl ProcessFixture {
    pub fn new(enrolled_name: Option<&str>) -> Self {
        let root = std::env::temp_dir().join(format!(
            "contextveil-process-{}-{}",
            std::process::id(),
            Canary::generate("FIXTURE").token()
        ));
        std::fs::create_dir_all(root.join("home").join("project")).expect("project directory");
        std::fs::create_dir_all(root.join("contextveil")).expect("config directory");
        std::fs::write(
            root.join("home").join("project").join(".contextveil.toml"),
            "version = 1\n",
        )
        .expect("project config");
        let fixture = Self { root };
        let global = enrolled_name.map_or_else(
            || "version = 1\n".to_string(),
            |name| format!("version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"{name}\"\n"),
        );
        fixture.write_global_config(&global);
        fixture
    }

    pub fn run(&self, arguments: &[&str], stdin: &[u8], variables: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_contextveil"));
        command
            .args(arguments)
            .current_dir(self.root.join("home").join("project"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.root.join("home"))
            .env("XDG_CONFIG_HOME", &self.root)
            .env("CLAUDE_PROJECT_DIR", self.root.join("home").join("project"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (name, value) in variables {
            command.env(name, value);
        }

        let mut child = command.spawn().expect("the contextveil binary runs");
        child
            .stdin
            .as_mut()
            .expect("stdin is piped")
            .write_all(stdin)
            .expect("write hook stdin");
        child.wait_with_output().expect("the hook finishes")
    }

    pub fn write_project_file(&self, relative: &str, contents: &str) -> PathBuf {
        let path = self.root.join("home").join("project").join(relative);
        std::fs::write(&path, contents).expect("write project file");
        path
    }

    pub fn project_dir(&self) -> PathBuf {
        self.root.join("home").join("project")
    }

    pub fn write_project_config(&self, contents: &str) {
        self.write_project_file(".contextveil.toml", contents);
    }

    pub fn write_global_config(&self, contents: &str) {
        std::fs::write(self.root.join("contextveil").join("config.toml"), contents)
            .expect("global config");
    }
}

impl Drop for ProcessFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
