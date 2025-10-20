use std::env;
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{self, ChildStdout, Command, Stdio};
use crossterm::terminal::{enable_raw_mode, disable_raw_mode};
use atty::Stream;

struct LineBuffer {
    buf: Vec<char>,
    cursor: usize,
    history: Vec<String>,
    history_cursor: usize,
    builtins: Vec<String>,
    hints: Vec<String>,
    in_tab_completion: bool,
}

impl LineBuffer {
    fn new() -> Self {
        Self { buf: vec![], cursor: 0, history: vec![], history_cursor: 0, builtins: vec![], hints: vec![], in_tab_completion: false }
    }

    fn clear_hints(&mut self) {
        self.hints = vec![];
        self.in_tab_completion = false;
    }

    fn set_builtins(&mut self, builtins: &[&str]) {
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

    fn tab_completion(&mut self) {
        let mut potential = vec![];
        for builtin in &self.builtins {
            if builtin.contains(&self.buf.iter().collect::<String>()) {
                potential.push(String::from(builtin));
            }
        }
        if potential.len() == 0 {
            let hints = find_executable_hints(&self.buf.iter().collect::<String>());
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

    fn read_line(&mut self, prompt: &str, interactive: bool) -> String {
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

fn split_args(input: &str) -> Vec<String> {
    let mut args = vec![];
    let mut current_arg = String::new();
    let mut in_whitespace = false;
    let mut in_single_quotes = false;
    let mut in_double_quotes = false;
    let mut last_backslash = false;
    let mut last_backslash_double_quote = false;

    for c in input.chars() {

        if in_single_quotes {
            if c == '\'' {
                in_single_quotes = false;
            } else {
                current_arg.push(c);
            }
            continue;
        }

        if in_double_quotes {
            if last_backslash_double_quote {
                if c != '\\' && c != '"' {
                    current_arg.push('\\');
                } 
            }
            if c == '\\' && !last_backslash_double_quote {
                last_backslash_double_quote = true;
                continue;
            }
            if c == '"' && !last_backslash_double_quote {
                in_double_quotes = false;
            } else {
                current_arg.push(c);
            }
            last_backslash_double_quote = false;
            continue;
        }

        if c == ' ' {
            if in_whitespace {
                continue;
            }
            args.push(current_arg);
            current_arg = String::new();
            in_whitespace = true;
            last_backslash = false;
            continue;
        } else if c == '\'' && !last_backslash {
            in_single_quotes = true;
        } else if c == '"' && !last_backslash {
            in_double_quotes = true;
        } else if c == '\\' {
            last_backslash = true;
            in_whitespace = false;
            continue;
        } else {
            current_arg.push(c);
        }
        in_whitespace = false;
        last_backslash = false;
    }
    if current_arg != "" {
        args.push(current_arg);
    }
    args
}

fn main() {
    let is_codecrafters = env::var("CODECRAFTERS_TEST_RUNNER_ID").is_ok();
    let interactive = atty::is(Stream::Stdout) && !is_codecrafters;
    let mut line_reader = LineBuffer::new();
    let mut input;
    let error_code;
    let builtins = ["echo", "exit", "type", "pwd", "cd", "history"];
    let mut current_dir = env::current_dir().unwrap();
    let mut history_appended = 0;
    let hist_file = env::var("HISTFILE").unwrap_or(String::from("~/.ssh_history"));
    let mut entries_read = 0;
    let mut passed_stdin: Option<ChildStdout> = None;
    let mut passed_stdin_builtin = String::new();
    let mut is_piped_in;
    let mut passed_args = vec![];
    let mut child_processes = vec![];

    line_reader.set_builtins(&builtins);

    //read history file
    let hist_file = PathBuf::from(hist_file);
    if hist_file.exists() {
        let hist_file_contents =  fs::read_to_string(&hist_file).unwrap();
        for hist_file_line in hist_file_contents.trim().split('\n') {
            if hist_file_line == "" {
                continue;
            }
            line_reader.insert_history_entry(hist_file_line, interactive);
            entries_read += 1;
        }
    }

    loop {
        let args;
        let mut redirect_stdout = None;
        let mut redirect_stderr = None;
        let mut appending_stdout = false;
        let mut appending_stderr = false;

        if passed_args.len() > 0 {
            input = String::new();
            args = passed_args;
            is_piped_in = true;
            passed_args = vec![];
        } else {
            input = line_reader.read_line("$ ", interactive);
            args = split_args(&input);
            is_piped_in = false;
        }
        if args.len() == 0 {
            continue;
        }
        let command = &args[0];

        let mut filtered_args = vec![];

        let mut skip_loop = false;
        let mut start_passing_args = false;
        for i in 0..args.len() {
            if skip_loop {
                continue;
            }
            let arg = String::from(&args[i]);
            if start_passing_args {
                passed_args.push(arg);
                continue;
            }
            if arg == "|" {
                start_passing_args = true;
                continue;
            }
            if redirect_stdout.is_none() &&  (arg == ">" || arg == "1>" || arg == ">>" || arg == "1>>") && args.len() > i + 1 {
                redirect_stdout = Some(String::from(&args[i+1]));
                if arg == ">>" || arg == "1>>" {
                    appending_stdout = true;
                }
                skip_loop = true;
            }
            if redirect_stderr.is_none() && (arg == "2>" || arg == "2>>") && args.len() > i + 1 {
                redirect_stderr = Some(String::from(&args[i+1]));
                skip_loop = true;
                if arg == "2>>" {
                    appending_stderr = true;
                }
            }
            if redirect_stderr.is_none() && redirect_stdout.is_none() && !start_passing_args {
                filtered_args.push(arg);
            }
        }
        let args = filtered_args;

        let mut my_stdout = String::new();
        let mut my_stderr = String::new();

        if !is_piped_in {
            let history_command = String::from(input.trim());
            line_reader.insert_history_entry(&history_command, interactive);
        }

        // handle commands
        if command == "exit" {
            if args.len() > 1 {
                error_code = i32::from_str_radix(&args[1], 10).unwrap_or(0);
            } else {
                error_code = 0;
            }
            break;
        } else if command == "echo" {
            my_stdout.push_str(&args[1..].join(" "));
            my_stdout.push('\n');
        } else if command == "type" {
            if args.len() == 1 {
                continue;
            }
            let mut found_builtin = false;
            for builtin in builtins {
                if builtin == args[1] {
                    my_stdout.push_str(&format!("{} is a shell builtin\n", args[1]));
                    found_builtin = true;
                    break;
                }
            }
            if !found_builtin {
                let result = find_executable(&args[1]);
                let found_executable = result.is_some();
    
                if found_executable {
                    let executable_path = result.unwrap();
                    my_stdout.push_str(&format!("{} is {}\n", args[1], executable_path));
                }
    
                if  !found_executable {
                    my_stderr.push_str(&format!("{}: not found\n", args[1]));
                }
            }
        } else if command == "pwd" {
            my_stdout.push_str(&format!("{}\n", current_dir.to_str().unwrap()));
        } else if command == "cd" {
            if args.len() == 1 {
                continue;
            }
            let mut path = PathBuf::from(&args[1]);
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
                    my_stderr.push_str(&format!("cd: {}: No such file or directory\n", args[1]));
                }
            } else if path.exists() {
                current_dir = path;
            } else {
                my_stderr.push_str(&format!("cd: {}: No such file or directory\n", args[1]));
            }
        } else if command == "history" {
            let history = line_reader.get_history();
            let mut start = 0;
            if args.len() > 1 {
                let result = usize::from_str_radix(&args[1], 10);
                if result.is_ok() {
                    start = history.len() - result.unwrap();
                } else {
                    let args = args[1..].to_vec();
                    if args.len() == 2 {
                        // read
                        if args[0] == "-r" {
                            let file_path = PathBuf::from(&args[1]);
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
                            let file_path = PathBuf::from(&args[1]);
                            let mut file = OpenOptions::new().create(true).write(true).open(file_path).unwrap();
                            for entry in history {
                                file.write_fmt(format_args!("{}\n", entry)).unwrap();
                            }
                        }
                        // append
                        else if args[0] == "-a" {
                            let file_path = PathBuf::from(&args[1]);
                            let mut file = OpenOptions::new().create(false).append(true).open(file_path).unwrap();
                            for entry in &history[history_appended..] {
                                file.write_fmt(format_args!("{}\n", entry)).unwrap();
                                history_appended += 1;
                            }
                        }
                    }
                    continue;
                }
            }
            for command_num in start..history.len() {
                my_stdout.push_str(&format!("    {}  {}\n", command_num + 1, history[command_num]));
            }
        } else { // executable commands
            let result = find_executable(&command);
            let found_executable = result.is_some();
            if found_executable {
                let executable_path = PathBuf::from(result.unwrap());
                let executable_path = executable_path.file_name().unwrap();
                let mut program;
                let stdin_config;
                let stdout_config;
                let stderr_config;
                let mut last_builtin = false;
                // configure stdin
                if is_piped_in && passed_stdin.is_some() {
                    stdin_config = Stdio::from(passed_stdin.unwrap());
                    passed_stdin = None;
                } else if is_piped_in && passed_stdin.is_none() {
                    stdin_config = Stdio::piped(); 
                    last_builtin = true;
                } else {
                    stdin_config = Stdio::inherit();
                }
                // configure stdout and stderr
                if passed_args.is_empty() && !redirect_stdout.is_some() && !redirect_stderr.is_some() {
                    stdout_config = Stdio::inherit();
                    stderr_config = Stdio::inherit();
                } else {
                    stdout_config = Stdio::piped();
                    stderr_config = Stdio::piped();
                }
                // start processes
                if args.len() == 1 {
                    program = Command::new(executable_path).current_dir(&current_dir).stdin(stdin_config).stdout(stdout_config).stderr(stderr_config).spawn().unwrap();
                } else {
                    let args_to_pass = args[1..].to_vec();
                    program = Command::new(executable_path).current_dir(&current_dir).args(args_to_pass).stdin(stdin_config).stdout(stdout_config).stderr(stderr_config).spawn().unwrap();
                }
                // handle builtins stdin
                if is_piped_in && last_builtin {
                    let process_stdin = program.stdin.as_mut().unwrap();
                    process_stdin.write_all(passed_stdin_builtin.as_bytes()).unwrap();
                    passed_stdin_builtin = "".into();
                }
                // handle stdout based on pipeline position
                if passed_args.len() > 0 {
                    let child_stdout = program.stdout.take();
                    passed_stdin = child_stdout;
                    child_processes.push(program);
                } else {
                    if redirect_stdout.is_some() || redirect_stderr.is_some() {
                        let output = program.wait_with_output().unwrap();
                        my_stdout.push_str(&String::from_utf8(output.stdout).unwrap_or("".into()));
                        my_stderr.push_str(&String::from_utf8(output.stderr).unwrap_or("".into()));
                    } else {
                        program.wait().unwrap();
                    }
                    // close all processes
                    for _ in 0..child_processes.len() {
                        let mut child = child_processes.pop().unwrap();
                        child.kill().unwrap();
                    }
                }
            } else {
                my_stderr.push_str(&format!("{}: command not found\n", command));
            }
        }
        
        // process stdout of builtins
        if passed_args.len() > 0 && builtins.contains(&command.as_str()) {
            passed_stdin_builtin = my_stdout;
            my_stdout = "".into();
        }

        if passed_args.len() > 0 {
        } else if redirect_stdout.is_some() {
            let stdout_file_path = PathBuf::from(redirect_stdout.unwrap());
            let mut file = OpenOptions::new().create(true).write(true).append(appending_stdout).truncate(!appending_stdout).open(stdout_file_path).unwrap();
            file.write(my_stdout.as_bytes()).unwrap();
        } else {
            print!("{}", &my_stdout);
        }
        if redirect_stderr.is_some() {
            let stderr_file_path = PathBuf::from(redirect_stderr.unwrap());
            let mut file = OpenOptions::new().create(true).write(true).append(appending_stderr).truncate(!appending_stderr).open(stderr_file_path).unwrap();
            file.write(my_stderr.as_bytes()).unwrap();
        } else {
            eprint!("{}", &my_stderr);
        }
    }
    let hist_dir = hist_file.parent();
    if error_code == 0 && (hist_dir.is_none() || hist_dir.unwrap().exists()) {
        let mut file = OpenOptions::new().create(true).append(true).open(hist_file).unwrap();
        let history = line_reader.get_history();
        for entry in &history[entries_read..] {
            file.write_fmt(format_args!("{}\n", entry)).unwrap();
        }
    }
    process::exit(error_code)
}
