use std::process::Command;

#[allow(dead_code)]
pub struct CompleteConfig {
    command: String,
    complete_file: String,
}

impl CompleteConfig {
    pub fn new(command: &str, complete_file: &str) -> Self {
        Self { command: command.to_string(), complete_file: complete_file.to_string() }
    }
    pub fn get_file(&self) -> String {
        self.complete_file.to_string()
    }
    pub fn get_output(&self) -> Vec<String> {
        if let Ok(output) = Command::new(&self.complete_file).output() {
            String::from_utf8(output.stdout).unwrap_or_default().split('\n').map(|s| s.to_string()).collect()
        } else {
            vec![]
        }
    }
}