pub enum DebuggerCommand {
    Quit,
    Run(Vec<String>),
    Continue,
    Backtrace,
    Breakpoint(String),
    Step,
    Next,
    Finish,
    Print(String)
}

impl DebuggerCommand {
    pub fn from_tokens(tokens: &Vec<&str>) -> Option<DebuggerCommand> {
        match tokens[0] {
            "q" | "quit" => Some(DebuggerCommand::Quit),
            "r" | "run" => {
                let args = tokens[1..].to_vec();
                Some(DebuggerCommand::Run(
                    args.iter().map(|s| s.to_string()).collect(),
                ))
            },
            "c" | "cont" | "continue" => Some(DebuggerCommand::Continue),
            "bt" | "back" | "backtrace" => Some(DebuggerCommand::Backtrace),
            "b" | "break" => Some(DebuggerCommand::Breakpoint(tokens.get(1).unwrap_or(&"").to_string())),
            "s" | "step" => Some(DebuggerCommand::Step),
            "n" | "next" => Some(DebuggerCommand::Next),
            "fin" | "finish" => Some(DebuggerCommand::Finish),
            "p" | "print" => Some(DebuggerCommand::Print(tokens.get(1).unwrap_or(&"").to_string())),
            // Default case:
            _ => None,
        }
    }
}
