use std::{collections::{HashMap}, env, fs, io::{self, Write}, os::unix::fs::PermissionsExt, path::PathBuf, rc::Rc, sync::RwLock};

use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

use crate::{modules::complete::CompleteConfig, read_key};

fn find_common_prefix(hints: &Vec<String>) -> String {
    if hints.len() == 0 {
        "".into()
    } else if hints.len() == 1 {
        hints[0].clone()
    } else {
        let mut common_prefix = String::from(&hints[0]);
        for hint in hints[1..].iter() {
            if !hint.contains(&common_prefix) {
                let mut new_common_prefix = String::new();
                for (c1, c2) in hint.chars().zip(common_prefix.chars()) {
                    if c1 == c2 {
                        new_common_prefix.push(c1);
                    } else {
                        break;
                    }
                }
                common_prefix = new_common_prefix;
            }
        }
        common_prefix.into()
    }
}

fn find_executable_hints(executable_name: &str) -> Vec<String> {
    let path_var = env::var("PATH").unwrap();
    let mut hints_found = vec![];
    for dir_name in path_var.split(":") {
        let dir_path = PathBuf::from(dir_name);
        if !dir_path.exists() {
            continue;
        }
        for entry in fs::read_dir(dir_path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.file_name().unwrap().to_str().unwrap().starts_with(executable_name) {
                let metadata = fs::metadata(&path).unwrap();
                let permissions = metadata.permissions();
                let mode = permissions.mode() as u16;
                let executable = 493u16;
                let is_executable = (mode & executable) == executable;
                if is_executable {
                    hints_found.push(String::from(path.to_str().unwrap()));
                }
            }
        }
    }
    hints_found
}

pub fn find_executable(executable_name: &str) -> Option<String> {
    let path_var = env::var("PATH").unwrap();
    for dir_name in path_var.split(":") {
        let dir_path = PathBuf::from(dir_name);
        if !dir_path.exists() {
            continue;
        }
        let exec_path = dir_path.join(executable_name);
        if !exec_path.exists() {
            continue;
        }
        let metadata = fs::metadata(&exec_path).unwrap();
        let permissions = metadata.permissions();
        let mode: u16 = permissions.mode() as u16;
        let executable: u16 = 493u16;
        let is_executable = (mode & executable) == executable;
        if is_executable {
            return Some(String::from(exec_path.to_str().unwrap()));
        }
    }
    return None;
}

fn find_file(dirname: &str, filename: &str) -> Vec<String> {
    let current_dir = env::current_dir().unwrap();
    let search_dir = current_dir.join(dirname);
    let mut files = vec![];

    for entry in fs::read_dir(search_dir).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name().into_string().unwrap().starts_with(filename) {
            files.push(entry.file_name().into_string().unwrap());
        }
    }
    files
}

pub struct LineBuffer {
    buf: Vec<char>,
    cursor: usize,
    history: Vec<String>,
    history_cursor: usize,
    builtins: Vec<String>,
    hints: Vec<String>,
    in_tab_completion: bool,
    custom_completes: Rc<RwLock<HashMap<String,CompleteConfig>>>
}

impl LineBuffer {
    pub fn new(custom_completes: Rc<RwLock<HashMap<String,CompleteConfig>>>) -> Self {
        Self { buf: vec![], cursor: 0, history: vec![], history_cursor: 0, builtins: vec![], hints: vec![], in_tab_completion: false, custom_completes }
    }

    fn clear_hints(&mut self) {
        self.hints = vec![];
        self.in_tab_completion = false;
    }

    pub fn set_builtins(&mut self, builtins: &[&str]) {
        for builtin in builtins {
            let builtin = String::from(*builtin);
            self.builtins.push(builtin);
        }
    }

    fn clear(&mut self) {
        self.buf = vec![];
        self.cursor = 0;
    }

    fn insert(&mut self, c: char) {
        self.buf.insert(self.cursor, c);
        self.cursor += 1;
    }

    pub fn insert_history_entry(&mut self, entry: &str, interactive: bool) {
        if self.history.len() == 0 || entry != self.history.last().unwrap() || !interactive {
            self.history.push(String::from(entry));
            self.history_cursor = self.history.len();
        }
    }

    pub fn get_history(&self) -> Vec<String> {
        self.history.clone()
    }

    fn delete_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.buf.remove(self.cursor);
        }
    }

    fn delete_right(&mut self) {
        if self.cursor < self.buf.len() {
            self.buf.remove(self.cursor);
        }
    }

    fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    fn move_right(&mut self) {
        if self.cursor < self.buf.len() {
            self.cursor += 1;
        }
    }

    fn tab_completion(&mut self) {
        let current_string = self.buf.iter().collect::<String>();

        // filename completion
        if let Some((pre, post)) = current_string.rsplit_once(' ') && !pre.ends_with('\\') {
            let command = pre.split(' ').next().unwrap_or_default();
            if let Some(config) = self.custom_completes.read().unwrap().get(command) {
                let completes = config.get_output(&current_string, self.cursor);
                let mut potential = vec![];
                for complete in completes {
                    if !complete.is_empty() && complete.starts_with(post) {
                        potential.push(complete.clone());
                    }
                }
                if potential.len() == 1 {
                    let to_complete = format!("{} {} ", pre, &potential[0]);
                    self.buf = to_complete.chars().collect();
                    self.cursor = self.buf.len();
                } else {
                    print!("\x07");
                    io::stdout().flush().unwrap();
                    if !potential.is_empty() {
                        let common_prefix = find_common_prefix(&potential);
                        if common_prefix != self.buf.iter().collect::<String>() {
                            self.buf = format!("{} {}", pre, common_prefix).chars().collect();
                            self.cursor = self.buf.len();
                        }
                        self.hints = potential;
                        self.in_tab_completion = true;
                    }
                }
            } else {
                let mut added_dir = String::new();
                let mut current_string = post;
                if let Some((current_dir, current_filepath)) = current_string.rsplit_once('/') {
                    added_dir = current_dir.to_string();
                    current_string = current_filepath;
                }
                let potential = find_file(&added_dir, current_string);
                if potential.len() == 1 {
                    let mut completed_path = potential[0].clone();
                    if !added_dir.is_empty() {
                        completed_path = format!("{}/{}", added_dir, completed_path);
                    }
                    let to_complete;
                    if PathBuf::from(completed_path.clone()).is_dir() {
                        to_complete = format!("{} {}/", pre, completed_path);
                    } else {
                        to_complete = format!("{} {} ", pre, completed_path);
                    }
                    self.buf = to_complete.chars().collect();
                    self.cursor = self.buf.len();
                } else {
                    print!("\x07");
                    io::stdout().flush().unwrap();
                    if potential.len() > 1 {
                        let mut hints = vec![];
                        for entry in potential {
                            let mut full_entry = entry;
                            if !added_dir.is_empty() {
                                full_entry = format!("{}/{}", added_dir, full_entry);
                            }
                            if PathBuf::from(&full_entry).is_dir() {
                                full_entry.push('/');
                            }
                            hints.push(full_entry);
                        }
                        let common_prefix = find_common_prefix(&hints);
                        let full_command_line = format!("{} {}", pre, common_prefix);
                        if full_command_line != self.buf.iter().collect::<String>() {
                            self.buf = full_command_line.chars().collect();
                            self.cursor = self.buf.len();
                        }
                        hints.sort();
                        self.hints = hints;
                        self.in_tab_completion = true;
                    }
                }
            }
        } else { // command completion
            let mut potential = vec![];
            for builtin in &self.builtins {
                if builtin.contains(&self.buf.iter().collect::<String>()) {
                    potential.push(String::from(builtin));
                }
            }
            if potential.len() == 0 {
                let hints = find_executable_hints(&current_string);
                for hint in hints {
                    let path = PathBuf::from(hint);
                    potential.push(String::from(path.file_name().unwrap().to_str().unwrap()));
                }
            }
            potential.sort();
            potential.dedup();
            if potential.len() == 1 {
                let mut to_complete = String::from(&potential[0]);
                to_complete.push(' ');
                self.buf = to_complete.chars().collect();
                self.cursor = self.buf.len();
            } else {
                print!("\x07");
                io::stdout().flush().unwrap();
                if potential.len() > 1 {
                    let common_prefix = find_common_prefix(&potential);
                    if common_prefix != self.buf.iter().collect::<String>() {
                        self.buf = common_prefix.chars().collect();
                        self.cursor = self.buf.len();
                    }
                    self.hints = potential;
                    self.in_tab_completion = true;
                }
            }
        }
    }

    fn tab_hints(&mut self) {
        println!("\n\r\x1B[K{}", self.hints.join("  "));
        self.clear_hints();
    }

    fn move_up_history(&mut self) {
        if self.history_cursor > 0 {
            let at_end = self.cursor == self.buf.len();
            self.history_cursor -= 1;
            self.buf = self.history[self.history_cursor].chars().collect();
            if self.cursor > self.buf.len() || at_end {
                self.cursor = self.buf.len();
            }
        }
    }

    fn move_down_history(&mut self) {
        if self.history_cursor < self.history.len() {
            let at_end = self.cursor == self.buf.len();
            self.history_cursor += 1;
            if self.history_cursor == self.history.len() {
                self.buf = vec![];
            } else {
                self.buf = self.history[self.history_cursor].chars().collect();
            }
            if self.cursor > self.buf.len() || at_end {
                self.cursor = self.buf.len();
            }
        }
    }

    fn render(&self, prompt: &str) {
        print!("\r\x1B[K{}{}", prompt, self.to_str());
        let diff = self.buf.len() - self.cursor;
        if diff > 0 {
            print!("\x1B[{}D", diff);
        }
        io::stdout().flush().unwrap();
    }

    pub fn read_line(&mut self, prompt: &str, interactive: bool) -> String {
        self.clear_hints();
        if interactive {
            print!("\r\x1B[K{}", prompt);
        } else {
            print!("{}", prompt)
        }
        io::stdout().flush().unwrap();
        enable_raw_mode().unwrap();
        self.clear();
        loop {
            let key = read_key();
            match key.as_str() {
                "\r" => break,
                "\n" => break,
                "left" => self.move_left(),
                "right" => self.move_right(),
                "up" => self.move_up_history(),
                "down" => self.move_down_history(),
                "\x7F" => self.delete_left(),
                "\x09" => {
                    if self.in_tab_completion {
                        self.tab_hints();
                    } else {
                        self.tab_completion()
                    }
                },
                "delete" => self.delete_right(),
                s if s.len() == 1 => self.insert(s.chars().next().unwrap()),
                _ => {}
            }
            if interactive {
                self.render(prompt);
            } else {
                if key == "up" || key == "down" {
                    print!("\r\x1B[K{}", prompt);
                    print!("{}", self.to_str());
                    io::stdout().flush().unwrap();
                } else if key == "\x09" { // tab
                    print!("\r\x1B[K{}", prompt);
                    print!("{}", self.to_str());
                    io::stdout().flush().unwrap();
                } else {
                    print!("{}", key);
                    io::stdout().flush().unwrap();
                }
            }
            if key != "\x09" {
                self.clear_hints();
            }
        }

        self.history_cursor = self.history.len();
        disable_raw_mode().unwrap();
        println!();
        self.to_str()
    }

    fn to_str(&self) -> String {
        self.buf.iter().collect::<String>()
    }
}