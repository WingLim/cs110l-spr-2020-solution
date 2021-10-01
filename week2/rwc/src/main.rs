use std::env;
use std::process;
use std::fs::File;
use std::io::{self, BufRead};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Too few arguments.");
        process::exit(1);
    }
    let filename = &args[1];
    
    let file = File::open(filename).expect("Invalid file!");
    let mut lines = Vec::new();

    for line in io::BufReader::new(file).lines() {
        let line_str = line.expect("Invalid line!");
        lines.push(line_str)
    }
    let lines_count = lines.len();

    let mut words_count = 0;
    let mut chars_count = 0;

    for line in lines {
        let splits = line.split_whitespace();
        for split in splits {
            words_count += 1;
            for _ in split.chars() {
                chars_count += 1;
            }
        }
    }

    println!("Your file has {} lines", lines_count);
    println!("Your file has {} words", words_count);
    println!("Your file has {} chars", chars_count);
}
