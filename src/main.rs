use std::{io::{self, Write}, process};

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
            if !found_builtin {
                println!("{}: not found", args);
            }
        } else {
            println!("{}: command not found", command);
        }
    }
    process::exit(error_code)
}
