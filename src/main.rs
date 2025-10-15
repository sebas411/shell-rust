use std::io::{self, Write};

fn main() {
    let mut input;
    loop {
        print!("$ ");
        io::stdout().flush().unwrap();
    
        // Wait for user input
        input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        println!("{}: command not found", input.trim());
    }
}
