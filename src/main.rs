use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{self, Command};

fn find_executable(executable_name: &str) -> Option<String> {
    let path_var = env::var("PATH").unwrap();
    for dir_name in path_var.split(":") {
        let result = fs::read_dir(dir_name);
        if result.is_err() {
            continue;
        }
        let files = result.unwrap();
        for file in files {
            let file = file.unwrap().path();
            if file.is_file() {
                let metadata = fs::metadata(&file).unwrap();
                let permissions = metadata.permissions();
                let mode: u16 = permissions.mode() as u16;
                let executable: u16 = 493u16;
                let is_executable = (mode & executable) == executable;
                if is_executable && file.file_name().unwrap() == executable_name {
                    return Some(String::from(file.to_str().unwrap()));
                }
            }
        }
    }
    return None;
}

fn main() {
    let mut input;
    let error_code;
    let builtins = ["echo", "exit", "type", "pwd", "cd"];
    let mut current_dir = env::current_dir().unwrap();
    loop {
        print!("$ ");
        io::stdout().flush().unwrap();
    
        // Wait for user input
        input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        let try_split = input.trim().split_once(' ');
        let command;
        let mut args: &str = &String::new();
    
        if try_split.is_some() {
            (command, args) = try_split.unwrap();
        } else {
            command = input.trim();
        }

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
        } else {
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
