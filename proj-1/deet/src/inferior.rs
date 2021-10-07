use iced_x86::{Decoder, DecoderOptions, Instruction, FastFormatter};
use nix::sys::ptrace;
use nix::sys::signal;
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;
use std::convert::TryInto;
use std::fs;
use std::mem::size_of;
use std::ops::BitAnd;
use std::os::unix::prelude::CommandExt;
use std::process::{Child, Command};
use std::collections::HashMap;
use crate::dwarf_data::{DwarfData, Line, Location, Variable};

pub enum Status {
    /// Indicates inferior stopped. Contains the signal that stopped the process, as well as the
    /// current instruction pointer that it is stopped at.
    Stopped(signal::Signal, usize),

    /// Indicates inferior exited normally. Contains the exit status code.
    Exited(i32),

    /// Indicates the inferior exited due to a signal. Contains the signal that killed the
    /// process.
    Signaled(signal::Signal),
}

/// This function calls ptrace with PTRACE_TRACEME to enable debugging on a process. You should use
/// pre_exec with Command to call this in the child process.
fn child_traceme() -> Result<(), std::io::Error> {
    ptrace::traceme().or(Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        "ptrace TRACEME failed",
    )))
}

fn align_addr_to_word(addr: usize) -> usize {
    addr & (-(size_of::<usize>() as isize) as usize)
}

pub struct Inferior {
    child: Child,
    pub breakpoints: HashMap<usize, u8>
}

impl Inferior {
    /// Attempts to start a new inferior process. Returns Some(Inferior) if successful, or None if
    /// an error is encountered.
    pub fn new(target: &str, args: &Vec<String>, breakpoints: &HashMap<usize, u8>) -> Option<Inferior> {
        let mut cmd = Command::new(target);
        cmd.args(args);
        unsafe {
            cmd.pre_exec(child_traceme);
        }
        let mut inferior = Inferior {
            child: cmd.spawn().ok()?,
            breakpoints: HashMap::new()
        };

        for addr in breakpoints.keys() {
            match inferior.write_byte(*addr, 0xcc) {
                Ok(orig_byte) => {
                    inferior.breakpoints.insert(*addr, orig_byte);
                },
                Err(_) => println!("Invalid breakpoint address {:#x}", addr),
            }
        }

        Some(inferior)
    }

    /// Returns the pid of this inferior.
    pub fn pid(&self) -> Pid {
        nix::unistd::Pid::from_raw(self.child.id() as i32)
    }

    /// Calls waitpid on this inferior and returns a Status to indicate the state of the process
    /// after the waitpid call.
    pub fn wait(&self, options: Option<WaitPidFlag>) -> Result<Status, nix::Error> {
        Ok(match waitpid(self.pid(), options)? {
            WaitStatus::Exited(_pid, exit_code) => Status::Exited(exit_code),
            WaitStatus::Signaled(_pid, signal, _core_dumped) => Status::Signaled(signal),
            WaitStatus::Stopped(_pid, signal) => {
                let regs = ptrace::getregs(self.pid())?;
                Status::Stopped(signal, regs.rip as usize)
            }
            other => panic!("waitpid returned unexpected status: {:?}", other),
        })
    }

    pub fn continue_run(&mut self) -> Result<Status, nix::Error> {
        self.step_over_breakpoint();
        // resume normal execution
        ptrace::cont(self.pid(), None)?;
        // wait for inferior to stop or terminate
        self.wait(None)
    }

    pub fn set_breakpoint(&mut self, addr: usize) {
        match self.child.try_wait() {
            Ok(None) => {
                match self.write_byte(addr, 0xcc) {
                    Ok(orig_byte) => {
                        if !self.breakpoints.contains_key(&addr) {
                            self.breakpoints.insert(addr, orig_byte);
                        }
                    }
                    Err(err) => println!("Failed to set breakpoint at {} with {}", addr, err),
                }
            },
            _ => {},
        };
    }

    #[allow(mutable_borrow_reservation_conflict)]
    fn remove_breakpoint(&mut self, addr: usize) {
        if let Some(orig_byte) = self.breakpoints.get(&addr) {
            self.write_byte(addr, *orig_byte).unwrap();
            self.breakpoints.remove(&addr);
        }
    }

    #[allow(mutable_borrow_reservation_conflict)]
    fn step_over_breakpoint(&mut self) {
        let mut regs = ptrace::getregs(self.pid()).unwrap();
        let rip = self.get_rip().unwrap();
        // if stopped at a breakpoint
        if let Some(orig_byte) = self.breakpoints.get(&(rip - 1)) {
            // restore the first byte of the instruction we replaced
            self.write_byte(rip - 1, *orig_byte).unwrap();
            // rewind the instruction pointer
            regs.rip = (rip - 1) as u64;
            ptrace::setregs(self.pid(), regs).unwrap();
            // go to next instruction
            ptrace::step(self.pid(), None).unwrap();
            // wait for inferior to stop due to SIGTRAP
            self.wait(None).unwrap();
            // restore 0xcc in the breakpoint location
            self.write_byte(rip - 1, 0xcc).unwrap();
        }
    }

    fn single_step_instruction(&mut self) {
        if self.breakpoints.contains_key(&self.get_rip().unwrap()) {
            self.step_over_breakpoint()
        } else {
            ptrace::step(self.pid(), None).unwrap();
            self.wait(None).unwrap();
        }
    }

    fn get_rip(&self) -> Result<usize, nix::Error> {
        let regs = ptrace::getregs(self.pid())?;
        Ok(regs.rip as usize)
    }

    pub fn step_in(&mut self, debug_data: &DwarfData) {
        let line = debug_data.get_line_from_addr(self.get_rip().unwrap()).unwrap();
        while debug_data.get_line_from_addr(self.get_rip().unwrap()).unwrap() == line {
            self.single_step_instruction();
        }
        
        let line_entry = debug_data.get_line_from_addr(self.get_rip().unwrap()).unwrap();
        if line_entry.number == line.number {
            self.print_assembly(line_entry.address);
        } else {
            self.print_source(&line_entry);
        }
    }

    pub fn step_out(&mut self) {
        let regs = ptrace::getregs(self.pid()).unwrap();
        let rbp = regs.rbp;
        let return_address = ptrace::read(self.pid(), (rbp + 8) as ptrace::AddressType).unwrap() as usize;

        let mut should_remove_breakpoint = false;
        if !self.breakpoints.contains_key(&return_address) {
            self.set_breakpoint(return_address);
            should_remove_breakpoint = true
        }

        self.continue_run().unwrap();

        if should_remove_breakpoint {
            self.remove_breakpoint(return_address);
        }
    }

    pub fn step_over(&mut self, debug_data: &DwarfData) -> Result<Status, nix::Error> {
        let func = debug_data.get_function(self.get_rip().unwrap()).unwrap();
        let func_entry = func.address;
        let func_end = func.address + func.text_length;

        let line = debug_data.get_line_from_addr(func_entry).unwrap();
        let mut line_number = line.number;
        let mut load_address = line.address;
        let start_line = debug_data.get_line_from_addr(self.get_rip().unwrap()).unwrap();
        let mut to_delete = Vec::new();

        while load_address < func_end {
            if load_address != start_line.address && !self.breakpoints.contains_key(&load_address) {
                self.set_breakpoint(load_address);
                to_delete.push(load_address);
            }
            line_number += 1;
            load_address = debug_data.get_addr_for_line(None, line_number).unwrap();
        }

        let regs = ptrace::getregs(self.pid())?;
        let rbp = regs.rbp;
        let return_address = (rbp + 8) as usize;
        if !self.breakpoints.contains_key(&return_address) {
            self.set_breakpoint(return_address);
            to_delete.push(return_address);
        }

        let status = self.continue_run()?;

        for addr in to_delete {
            self.remove_breakpoint(addr);
        }

        Ok(status)
    }

    pub fn kill(&mut self) {
        self.child.kill().unwrap();
        self.wait(None).unwrap();
        println!("Killing running inferior (pid {})", self.pid());
    }

    pub fn print_backtrace(&self, debug_data: &DwarfData) -> Result<(), nix::Error> {
        let regs = ptrace::getregs(self.pid())?;
        let mut rip = regs.rip as usize;
        let mut rbp = regs.rbp as usize;

        loop {
            let line = debug_data.get_line_from_addr(rip).unwrap();
            let func = debug_data.get_function_from_addr(rip).unwrap();
            println!("{} ({})", func, line);

            if func == "main" {
                break;
            }

            rip = ptrace::read(self.pid(), (rbp + 8) as ptrace::AddressType)? as usize;
            rbp = ptrace::read(self.pid(), rbp as ptrace::AddressType)? as usize;
        }

        Ok(())
    }

    pub fn print_source(&self, line: &Line) {
        let path = line.file.clone();
        if let Ok(source) = fs::read_to_string(path) {
            let line = source.lines().nth(line.number - 1).unwrap();

            println!("{}", line);
        }
    }

    pub fn print_assembly(&self, addr: usize) {
        let data = ptrace::read(self.pid(), addr as ptrace::AddressType).unwrap();
        let bytes = data.to_ne_bytes();
        let mut decoder = Decoder::new(64, &bytes, DecoderOptions::NONE);
        let mut formatter = FastFormatter::new();
        let mut output = String::new();
        let mut instruction = Instruction::default();

        while decoder.can_decode() {
            decoder.decode_out(&mut instruction);
            output.clear();
            formatter.format(&instruction, &mut output);

            println!(" {}", output);
        }
    }

    fn get_variable_value(&self, var: &Variable, name: &String) {
        let addr;
        match var.location {
            Location::Address(address) => {
                addr = address;
            },
            Location::FramePointerOffset(offset) => {
                let regs = ptrace::getregs(self.pid()).unwrap();
                let rbp = regs.rbp;
                addr = ((rbp as isize) + offset + 16) as usize;
            },
        }
        let raw_data = ptrace::read(self.pid(), addr as ptrace::AddressType).unwrap();
        match var.entity_type.name.as_str() {
            "int" => {
                let data = raw_data.bitand(0xFFFFFFFF) as i32;
                println!("{} :{} = {}", name, var.entity_type, data);
            }
            "long int" => {
                println!("{} :{} = {}", name, var.entity_type, raw_data);
            }
            "float" => {
                let mut data_32_bytes = raw_data.bitand(0xFFFFFFFF).to_be_bytes().to_vec();
                data_32_bytes.retain(|&x| x != 0);
                let data = f32::from_be_bytes(data_32_bytes.try_into().unwrap());
                println!("{} :{} = {}", name, var.entity_type, data);
            }
            "double" => {
                let data_bytes = raw_data.to_be_bytes().to_vec();
                let data = f64::from_be_bytes(data_bytes.try_into().unwrap());
                println!("{} :{} = {}", name, var.entity_type, data);
            }
            _ => {
                println!("Error type: \"{}\" not support yet.", var.entity_type);
            }
        }
    }

    pub fn print_variable(&self, debug_data: &DwarfData, name: String) {
        let rip = self.get_rip().unwrap();
        let func = debug_data.get_function(rip).unwrap();
        let mut have_var = false;
        for var in &func.variables {
            if var.name == name {
                have_var = true;
                self.get_variable_value(var, &name);
            }
        }
        for var in debug_data.get_global_variables() {
            if var.name == name {
                have_var = true;
                self.get_variable_value(var, &name);
            }
        }
        if !have_var {
            println!("Error no such variable");
        }
    }

    pub fn write_byte(&mut self, addr: usize, val: u8) -> Result<u8, nix::Error> {
        let aligned_addr = align_addr_to_word(addr);
        let byte_offset = addr - aligned_addr;
        let word = ptrace::read(self.pid(), aligned_addr as ptrace::AddressType)? as u64;
        let orig_byte = (word >> 8 * byte_offset) & 0xff;
        let masked_word = word & !(0xff << 8 * byte_offset);
        let updated_word = masked_word | ((val as u64) << 8 * byte_offset);
        ptrace::write(
            self.pid(),
            aligned_addr as ptrace::AddressType,
            updated_word as *mut std::ffi::c_void
        )?;

        Ok(orig_byte as u8)
    }
}
