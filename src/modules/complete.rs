use std::process::Command;

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
    pub fn get_output(&self, line: &str, cursor: usize) -> Vec<String> {
        let mut split_line = line.split(' ').collect::<Vec<_>>();
        split_line.remove(0);
        let current = split_line.pop().unwrap_or_default();
        let past = split_line.pop().unwrap_or_default();

        let mut process = Command::new(&self.complete_file);
        process.args([&self.command, current, past]).envs([("COMP_LINE", line), ("COMP_POINT", &format!("{}", cursor))]);
        
        if let Ok(output) = process.output() {
            String::from_utf8(output.stdout).unwrap_or_default().split('\n').map(|s| s.to_string()).collect()
        } else {
            vec![]
        }
    }
}