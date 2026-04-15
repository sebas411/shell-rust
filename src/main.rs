use std::collections::{BTreeSet, HashMap};
use std::env;
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{self, Child, ChildStdout, Command, Stdio};
use std::rc::Rc;
use std::sync::RwLock;
use atty::Stream;

use crate::modules::line_buffer::{LineBuffer, find_executable};
mod modules;

struct BgJobList {
    jobs: HashMap<usize, Rc<RwLock<BackgroundJob>>>,
    jobs_ages: Vec<usize>,
    current_job: usize,
    recyclable: BTreeSet<usize>,
}

impl BgJobList {
    fn new() -> Self {
        Self { jobs: HashMap::new(), current_job: 0, jobs_ages: Vec::new(), recyclable: BTreeSet::new() }
    }
    fn insert_job(&mut self, job: BackgroundJob) -> usize {
        let internal_id;
        if let Some(recycled) = self.recyclable.pop_first() {
            internal_id = recycled;
        } else {
            self.current_job += 1;
            internal_id = self.current_job;
        }
        self.jobs_ages.push(internal_id);
        self.jobs.insert(internal_id, Rc::new(RwLock::new(job)));
        internal_id
    }
    fn get_recent(&self) -> (usize, usize) { // (latest, second)
        let jobs_len = self.jobs_ages.len();
        if jobs_len >= 2 {
            (self.jobs_ages[jobs_len-1], self.jobs_ages[jobs_len-2])
        } else if jobs_len == 1 {
            (self.jobs_ages[0], 0)
        } else {
            (0, 0)
        }
    }
    fn reap(&mut self, print_reaped: bool) {
        let mut to_remove = vec![];
        for (i_id, job) in &self.jobs {
            if job.write().unwrap().get_status() == "Done" {
                to_remove.push(*i_id);
            }
        }
        let (latest, second) = self.get_recent();
        for internal_id in &to_remove {
            let old = self.jobs.remove(&internal_id);
            if print_reaped && let Some(old) = old {
                let age = match *internal_id {
                    n if n == latest => '+',
                    n if n == second => '-',
                    _ => ' ',
                };
                println!("[{}]{}  Done                    {}", internal_id, age, old.read().unwrap().get_command());
            }
            let age_position = self.jobs_ages.iter().position(|i_id| i_id == internal_id).unwrap();
            self.jobs_ages.remove(age_position);
            self.recyclable.insert(*internal_id);
        }
    }
}

impl<'a> IntoIterator for &'a BgJobList {
    type Item = (&'a usize, &'a Rc<RwLock<BackgroundJob>>);
    type IntoIter = std::collections::hash_map::Iter<'a, usize, Rc<RwLock<BackgroundJob>>>;

    fn into_iter(self) -> Self::IntoIter {
        self.jobs.iter()
    }
}

struct BackgroundJob {
    command: String,
    job: Child,
}

impl BackgroundJob {
    fn new(command: &str, job: Child) -> Self {
        Self { command: command.to_string(), job }
    }
    fn get_command(&self) -> String {
        self.command.clone()
    }
    fn get_status(&mut self) -> String {
        match self.job.try_wait() {
            Ok(Some(_)) => "Done".to_string(),
            Ok(None) => "Running".to_string(),
            Err(_) => "".to_string(),
        }
    }
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
            if last_backslash {
                last_backslash = false;
                current_arg.push('\\');
            } else {
                last_backslash = true;
            }
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
    let builtins = ["echo", "exit", "type", "pwd", "cd", "history", "jobs"];
    let mut current_dir = env::current_dir().unwrap();
    let mut history_appended = 0;
    let hist_file = env::var("HISTFILE").unwrap_or(String::from("~/.sebash_history"));
    let mut entries_read = 0;
    let mut passed_stdin: Option<ChildStdout> = None;
    let mut passed_stdin_builtin = String::new();
    let mut is_piped_in;
    let mut passed_args = vec![];
    let mut child_processes = vec![];
    let mut is_background = false;
    let mut bg_jobs = BgJobList::new();

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
        bg_jobs.reap(true);

        let mut args;
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
            if let Some(last) = args.last() && last == "&" {
                args.pop();
                is_background = true;
            } else {
                is_background = false;
            }
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
        match command.as_str() {
            "exit" => {
                if args.len() > 1 {
                    error_code = i32::from_str_radix(&args[1], 10).unwrap_or(0);
                } else {
                    error_code = 0;
                }
                break;
            }
            "echo" => {
                my_stdout.push_str(&args[1..].join(" "));
                my_stdout.push('\n');
            }
            "type" => {
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
            }
            "pwd" => {
                my_stdout.push_str(&format!("{}\n", current_dir.to_str().unwrap()));
            }
            "cd" => {
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
            }
            "history" => {
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
            }
            "jobs" => {
                let jobs = &bg_jobs.into_iter().collect::<Vec<_>>();
                let mut jobs = jobs.clone();
                jobs.sort_by(|a, b| a.0.cmp(b.0));
                let (latest, second) = &bg_jobs.get_recent();
                for (internal_id, job) in jobs {
                    let age = match internal_id {
                        n if n == latest => '+',
                        n if n == second => '-',
                        _ => ' ',
                    };
                    let status = job.write().unwrap().get_status();
                    my_stdout.push_str(&format!("[{}]{}  {:24}{}\n", internal_id, age, status, job.read().unwrap().get_command()));
                }
                bg_jobs.reap(false);
            }
            _ => {
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
                            if is_background {
                                let pid = program.id();
                                let job = BackgroundJob::new(&args.join(" "), program);
                                let internal_id = bg_jobs.insert_job(job);
                                my_stdout = format!("[{}] {}\n", internal_id, pid);
                            } else {
                                let output = program.wait_with_output().unwrap();
                                my_stdout.push_str(&String::from_utf8(output.stdout).unwrap_or_default());
                                my_stderr.push_str(&String::from_utf8(output.stderr).unwrap_or_default());
                            }
                        } else {
                            if is_background {
                                let pid = program.id();
                                let job = BackgroundJob::new(&args.join(" "), program);
                                let internal_id = bg_jobs.insert_job(job);
                                my_stdout = format!("[{}] {}\n", internal_id, pid);
                            } else {
                                program.wait().unwrap();
                            }
                        }
                        
                        if !is_background {
                            // close all processes
                            for _ in 0..child_processes.len() {
                                let mut child = child_processes.pop().unwrap();
                                child.kill().unwrap();
                            }
                        }
                    }
                } else {
                    my_stderr.push_str(&format!("{}: command not found\n", command));
                }
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
