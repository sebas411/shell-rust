use std::{env, fs, io::{self, Write}, os::unix::fs::PermissionsExt, process::{self, Command}};

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
    let builtins = ["echo", "exit", "type"];
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
        } else {
            let result = find_executable(command);
            let found_executable = result.is_some();
            if found_executable {
                let executable_path = result.unwrap();
                let output;
                if args == "" {
                    output = Command::new(executable_path).output().unwrap();
                } else {
                    let args_to_pass = args.split(' ').collect::<Vec<&str>>();
                    output = Command::new(executable_path).args(args_to_pass).output().unwrap();
                }
                println!("{}", String::from_utf8(output.stdout).unwrap());
            } else {
                println!("{}: command not found", command);
            }
        }
    }
    process::exit(error_code)
}
