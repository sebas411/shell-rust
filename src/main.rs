use std::env;
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{self, Write, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{self, Command};
use crossterm::terminal::{enable_raw_mode, disable_raw_mode};
use atty::Stream;

struct LineBuffer {
    buf: Vec<char>,
    cursor: usize,
    history: Vec<String>,
    history_cursor: usize,
}

impl LineBuffer {
    fn new() -> Self {
        Self { buf: vec![], cursor: 0, history: vec![], history_cursor: 0 }
    }

    fn clear(&mut self) {
        self.buf = vec![];
        self.cursor = 0;
    }

    fn insert(&mut self, c: char) {
        self.buf.insert(self.cursor, c);
        self.cursor += 1;
    }

    fn insert_history_entry(&mut self, entry: &str, interactive: bool) {
        if self.history.len() == 0 || entry != self.history.last().unwrap() || !interactive {
            self.history.push(String::from(entry));
            self.history_cursor = self.history.len();
        }
    }

    fn get_history(&self) -> Vec<String> {
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
        if self.history_cursor < self.buf.len() {
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

    fn read_line(&mut self, prompt: &str, interactive: bool) -> String {
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
                } else {
                    print!("{}", key);
                }
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

fn find_executable(executable_name: &str) -> Option<String> {
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

fn read_key() -> String {
    let mut stdin = std::io::stdin();
    let mut buf = [0; 3];
    stdin.read_exact(&mut buf[..1]).unwrap();

    if buf[0] == 0x1B {
        // Possible escape sequence
        if stdin.read(&mut buf[1..]).unwrap_or(0) == 2 {
            match &buf {
                [0x1B, 0x5B, 0x41] => return "up".into(),
                [0x1B, 0x5B, 0x42] => return "down".into(),
                [0x1B, 0x5B, 0x43] => return "right".into(),
                [0x1B, 0x5B, 0x44] => return "left".into(),
                [0x1B, 0x5B, 0x33] => {
                    stdin.read_exact(&mut buf[..1]).unwrap();
                    if buf[0] == 0x7E {
                        return "delete".into()
                    } else {
                        return "escape".into()
                    }
                },
                _ => return "escape".into(),
            }
        } else {
            return "escape".into();
        }
    }
    (buf[0] as char).to_string()
}

fn main() {
    let is_codecrafters = env::var("CODECRAFTERS_TEST_RUNNER_ID").is_ok();
    let interactive = atty::is(Stream::Stdout) && !is_codecrafters;
    let mut line_reader = LineBuffer::new();
    let mut input;
    let error_code;
    let builtins = ["echo", "exit", "type", "pwd", "cd", "history"];
    let mut current_dir = env::current_dir().unwrap();

    loop {
        // Wait for user input
        input = line_reader.read_line("$ ", interactive);

        let try_split = input.trim().split_once(' ');
        let command;
        let mut args: &str = &String::new();
    
        if try_split.is_some() {
            (command, args) = try_split.unwrap();
        } else {
            command = input.trim();
        }
        let history_command = String::from(input.trim());
        line_reader.insert_history_entry(&history_command, interactive);

        if command == "exit" {
            if args != "" {
                error_code = i32::from_str_radix(args, 10).unwrap();
            } else {
                error_code = 0;
            }
            break;
        } else if command == "echo" {
            println!("{}", args);
        } else if command == "type" {
            let mut found_builtin = false;
            for builtin in builtins {
                if builtin == args {
                    println!("{} is a shell builtin", args);
                    found_builtin = true;
                    break;
                }
            }
            if found_builtin {
                continue;
            }
            let result = find_executable(args);
            let found_executable = result.is_some();

            if found_executable {
                let executable_path = result.unwrap();
                println!("{} is {}", args, executable_path);
            }

            if  !found_executable {
                println!("{}: not found", args);
            }
        } else if command == "pwd" {
            println!("{}", current_dir.to_str().unwrap());
        } else if command == "cd" {
            let mut path = PathBuf::from(args);
            if path.iter().nth(0).unwrap() == "~" {
                let old_path = path.clone();
                path = PathBuf::from(env::var("HOME").unwrap());
                let sub_dir_vec: Vec<&OsStr> = old_path.iter().skip(1).collect();
                for d in sub_dir_vec {
                    path = path.join(d);
                }
            }
            if path.is_relative() {
                let mut path_built: PathBuf = current_dir.clone();
                for part in path.iter() {
                    if part == "." {
                        path_built = current_dir.clone();
                    } else if part == ".." {
                        path_built.pop();
                    } else {
                        path_built = path_built.join(part);
                    }
                }
                if path_built.exists() {
                    current_dir = path_built;
                } else {
                    println!("cd: {}: No such file or directory", args);
                }
            } else if path.exists() {
                current_dir = path;
            } else {
                println!("cd: {}: No such file or directory", args);
            }
        } else if command == "history" {
            let history = line_reader.get_history();
            let mut start = 0;
            if args != "" {
                let result = usize::from_str_radix(args, 10);
                if result.is_ok() {
                    start = history.len() - result.unwrap();
                } else {
                    let args = args.split(' ').collect::<Vec<&str>>();
                    if args.len() == 2 {
                        // read
                        if args[0] == "-r" {
                            let file_path = PathBuf::from(args[1]);
                            if file_path.exists() {
                                let file_contents = fs::read_to_string(file_path).unwrap();
                                for file_line in file_contents.split('\n') {
                                    if file_line != "" {
                                        line_reader.insert_history_entry(file_line, interactive);
                                    }
                                }
                            }
                        }
                        // write
                        else if args[0] == "-w" {
                            let file_path = PathBuf::from(args[1]);
                            let mut file = OpenOptions::new().create(true).write(true).open(file_path).unwrap();
                            for entry in history {
                                file.write_fmt(format_args!("{}\n", entry)).unwrap();
                            }
                        }
                    }
                    continue;
                }
            }
            for command_num in start..history.len() {
                println!("    {}  {}", command_num + 1, history[command_num]);
            }
        } else { // executable commands
            let result = find_executable(command);
            let found_executable = result.is_some();
            if found_executable {
                let executable_path = PathBuf::from(result.unwrap());
                let executable_path = executable_path.file_name().unwrap();
                let output;
                if args == "" {
                    output = Command::new(executable_path).output().unwrap();
                } else {
                    let args_to_pass = args.split(' ').collect::<Vec<&str>>();
                    output = Command::new(executable_path).args(args_to_pass).output().unwrap();
                }
                print!("{}", String::from_utf8(output.stdout).unwrap());
            } else {
                println!("{}: command not found", command);
            }
        }
    }
    process::exit(error_code)
}
