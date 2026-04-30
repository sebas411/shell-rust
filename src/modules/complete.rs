
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
}