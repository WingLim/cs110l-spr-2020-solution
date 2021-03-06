use crate::debugger_command::DebuggerCommand;
use crate::inferior::{Inferior, Status};
use crate::dwarf_data::{DwarfData, Error as DwarfError};
use rustyline::error::ReadlineError;
use rustyline::Editor;
use std::collections::HashMap;

pub struct Debugger {
    target: String,
    history_path: String,
    readline: Editor<()>,
    inferior: Option<Inferior>,
    debug_data: DwarfData,
    breakpoints: HashMap<usize, u8>
}

impl Debugger {
    /// Initializes the debugger.
    pub fn new(target: &str) -> Debugger {
        let debug_data = match DwarfData::from_file(target) {
            Ok(val) => val,
            Err(DwarfError::ErrorOpeningFile) => {
                println!("Could not open file {}", target);
                std::process::exit(1);
            }
            Err(DwarfError::DwarfFormatError(err)) => {
                println!("Could not debugging symbols from {}: {:?}", target, err);
                std::process::exit(1);
            }
        };
        debug_data.print();

        let history_path = format!("{}/.deet_history", std::env::var("HOME").unwrap());
        let mut readline = Editor::<()>::new();
        // Attempt to load history from ~/.deet_history if it exists
        let _ = readline.load_history(&history_path);
        let breakpoints = HashMap::new();

        Debugger {
            target: target.to_string(),
            history_path,
            readline,
            inferior: None,
            debug_data,
            breakpoints
        }
    }

    pub fn run(&mut self) {
        loop {
            match self.get_next_command() {
                DebuggerCommand::Run(args) => {
                    if self.inferior.is_some() {
                        self.inferior.as_mut().unwrap().kill();
                        self.inferior = None;
                    }
                    if let Some(inferior) = Inferior::new(&self.target, &args, &self.breakpoints) {
                        // Create the inferior
                        self.inferior = Some(inferior);
                        
                        let status = self.inferior.as_mut().unwrap().continue_run();
                        self.check_status(status);
                    } else {
                        println!("Error starting subprocess");
                    }
                }
                DebuggerCommand::Continue => {
                    if self.inferior.is_some() {
                        let status = self.inferior.as_mut().unwrap().continue_run();
                        self.check_status(status);
                    } else {
                        println!("Error no inferior running");
                    }
                }
                DebuggerCommand::Quit => {
                    if self.inferior.is_some() {
                        self.inferior.as_mut().unwrap().kill();
                        self.inferior = None;
                    }
                    return;
                }
                DebuggerCommand::Backtrace => {
                    if self.inferior.is_some() {
                        self.inferior.as_mut().unwrap().print_backtrace(&self.debug_data).unwrap();
                    } else {
                        println!("Error no inferior running")
                    }
                }
                DebuggerCommand::Breakpoint(location) => {
                    let bp_addr;
                    if location.starts_with("*") {
                        if let Some(address) = self.parse_address(&location[1..]) {
                            bp_addr = address;
                        } else {
                            println!("Invalid address");
                            continue;
                        }
                    } else if let Some(line_number) = usize::from_str_radix(&location, 10).ok() {
                        if let Some(address) = self.debug_data.get_addr_for_line(None, line_number) {
                            bp_addr = address;
                        } else {
                            println!("Invalid line number");
                            continue;
                        }
                    } else if let Some(address) = self.debug_data.get_addr_for_function(None, &location) {
                        bp_addr = address;
                    } else {
                        println!("Usage: b|break|breakpoint *address|line|func");
                        continue;
                    }
                    
                    
                    if self.inferior.is_some() {
                        println!("Set breakpoint {} at {:#x}", self.inferior.as_mut().unwrap().breakpoints.len(), bp_addr);
                        self.inferior.as_mut().unwrap().set_breakpoint(bp_addr);
                    } else {
                        println!("Set breakpoint {} at {:#x}", self.breakpoints.len(), bp_addr);
                        self.breakpoints.insert(bp_addr, 0);
                    }
                }
                DebuggerCommand::Step => {
                    if self.inferior.is_some() {
                        self.inferior.as_mut().unwrap().step_in(&self.debug_data);
                    } else {
                        println!("Error no inferior running");
                    }
                }
                DebuggerCommand::Next => {
                    if self.inferior.is_some() {
                        let status = self.inferior.as_mut().unwrap().step_over(&self.debug_data);
                        self.check_status(status);
                    } else {
                        println!("Error no inferior running");
                    }
                }
                DebuggerCommand::Finish => {
                    if self.inferior.is_some() {
                        let status = self.inferior.as_mut().unwrap().step_out();
                        self.check_status(status);
                    } else {
                        println!("Error no inferior running");
                    }
                }
                DebuggerCommand::Print(name) => {
                    if self.inferior.is_some() {
                        self.inferior.as_mut().unwrap().print_variable(&self.debug_data, name);
                    } else {
                        println!("Error no inferior running");
                    }
                }
            }
        }
    }

    fn parse_address(&self, addr: &str) -> Option<usize> {
        let addr_without_0x = if addr.to_lowercase().starts_with("0x") {
            &addr[2..]
        } else {
            &addr
        };
        usize::from_str_radix(addr_without_0x, 16).ok()
    }

    fn check_status(&mut self, status: Result<Status, nix::Error>) {
        match status.unwrap() {
            Status::Stopped(signal, rip) => {
                println!("Child stopped (signal {})", signal);
                match self.debug_data.get_line_from_addr(rip) {
                    Some(line) => {
                        println!("Stopped at {}", line);
                        self.inferior.as_mut().unwrap().print_source(&line);
                    },
                    None => {
                        println!("Stopped at {:#x}", rip)
                    },
                }
            },
            Status::Exited(exit_code) => {
                println!("Child exited (status {})", exit_code);
                self.inferior = None;
            },
            Status::Signaled(signal) => {
                println!("Child exited (signal {})", signal);
                self.inferior = None;
            },
        }
    }

    /// This function prompts the user to enter a command, and continues re-prompting until the user
    /// enters a valid command. It uses DebuggerCommand::from_tokens to do the command parsing.
    ///
    /// You don't need to read, understand, or modify this function.
    fn get_next_command(&mut self) -> DebuggerCommand {
        loop {
            // Print prompt and get next line of user input
            match self.readline.readline("(deet) ") {
                Err(ReadlineError::Interrupted) => {
                    // User pressed ctrl+c. We're going to ignore it
                    println!("Type \"quit\" to exit");
                }
                Err(ReadlineError::Eof) => {
                    // User pressed ctrl+d, which is the equivalent of "quit" for our purposes
                    return DebuggerCommand::Quit;
                }
                Err(err) => {
                    panic!("Unexpected I/O error: {:?}", err);
                }
                Ok(line) => {
                    if line.trim().len() == 0 {
                        continue;
                    }
                    self.readline.add_history_entry(line.as_str());
                    if let Err(err) = self.readline.save_history(&self.history_path) {
                        println!(
                            "Warning: failed to save history file at {}: {}",
                            self.history_path, err
                        );
                    }
                    let tokens: Vec<&str> = line.split_whitespace().collect();
                    if let Some(cmd) = DebuggerCommand::from_tokens(&tokens) {
                        return cmd;
                    } else {
                        println!("Unrecognized command.");
                    }
                }
            }
        }
    }
}
