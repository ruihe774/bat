use std::env;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::process::Command;

use tempfile::TempDir;

pub struct BatTester {
    /// Temporary working directory
    temp_dir: TempDir,

    /// Path to the *bat* executable
    exe: PathBuf,
}

impl BatTester {
    pub fn test_snapshot(&self, name: &str, style: &str) {
        let output = Command::new(&self.exe)
            .current_dir(self.temp_dir.path())
            .args(&[
                "sample.rs",
                "--no-config",
                "--paging=never",
                "--color=never",
                "--decorations=always",
                "--terminal-width=80",
                &format!("--style={}", style),
            ])
            .output()
            .expect("bat failed");

        // have to do the replace because the filename in the header changes based on the current working directory
        let actual = String::from_utf8_lossy(&output.stdout)
            .as_ref()
            .replace("tests/snapshots/", "");

        let mut expected = String::new();
        let mut file = File::open(format!("tests/snapshots/output/{}.snapshot.txt", name))
            .expect("snapshot file missing");
        file.read_to_string(&mut expected)
            .expect("could not read snapshot file");

        assert_eq!(expected, actual);
    }
}

impl Default for BatTester {
    fn default() -> Self {
        let temp_dir = TempDir::new().expect("Temp directory");

        let root = env::current_exe()
            .expect("tests executable")
            .parent()
            .expect("tests executable directory")
            .parent()
            .expect("bat executable directory")
            .to_path_buf();

        let exe_name = if cfg!(windows) { "bat.exe" } else { "bat" };
        let exe = root.join(exe_name);

        BatTester { temp_dir, exe }
    }
}
