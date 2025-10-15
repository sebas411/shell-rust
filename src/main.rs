use std::{io::{self, Write}, process};

fn main() {
    let mut input;
    let error_code;
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
        } else {
            println!("{}: command not found", command);
        }
    }
    process::exit(error_code)
}
