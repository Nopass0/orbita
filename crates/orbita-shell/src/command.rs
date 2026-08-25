use alloc::string::String;
use alloc::vec::Vec;

/// A single token from the shell input stream.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CommandWord {
    pub text: String,
    pub quoted: bool,
    pub expand: bool,
}

impl CommandWord {
    pub fn new(text: impl Into<String>, quoted: bool, expand: bool) -> Self {
        Self {
            text: text.into(),
            quoted,
            expand,
        }
    }
}

/// Command name, kept separate from the rest of the argument vector.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CommandName {
    pub word: String,
}

impl CommandName {
    pub fn new(word: impl Into<String>) -> Self {
        Self { word: word.into() }
    }
}

/// Command argument in parsed shell form.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CommandArg {
    pub word: String,
    pub quoted: bool,
    pub expand: bool,
}

impl CommandArg {
    pub fn new(word: impl Into<String>, quoted: bool, expand: bool) -> Self {
        Self {
            word: word.into(),
            quoted,
            expand,
        }
    }
}

/// Parsed command line, ready for dispatch to a builtin or external command.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CommandLine {
    pub name: CommandName,
    pub args: Vec<CommandArg>,
}

impl CommandLine {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: CommandName::new(name),
            args: Vec::new(),
        }
    }

    pub fn push_arg(&mut self, arg: impl Into<String>, quoted: bool, expand: bool) {
        self.args.push(CommandArg::new(arg, quoted, expand));
    }

    pub fn arg_count(&self) -> usize {
        self.args.len()
    }
}

/// Parsed representation that preserves the original text for diagnostics.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ParsedCommand {
    pub raw: String,
    pub line: CommandLine,
}

impl ParsedCommand {
    pub fn new(raw: impl Into<String>, line: CommandLine) -> Self {
        Self {
            raw: raw.into(),
            line,
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum RedirectKind {
    Input,
    Output,
    Append,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RedirectSpec {
    pub kind: RedirectKind,
    pub target: CommandArg,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ShellAssignment {
    pub name: String,
    pub value: CommandArg,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SimpleCommand {
    pub assignments: Vec<ShellAssignment>,
    pub name: Option<CommandName>,
    pub args: Vec<CommandArg>,
    pub redirects: Vec<RedirectSpec>,
}

impl SimpleCommand {
    pub fn new() -> Self {
        Self {
            assignments: Vec::new(),
            name: None,
            args: Vec::new(),
            redirects: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.assignments.is_empty()
            && self.name.is_none()
            && self.args.is_empty()
            && self.redirects.is_empty()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CommandPipeline {
    pub commands: Vec<SimpleCommand>,
}

impl CommandPipeline {
    pub fn new() -> Self {
        Self { commands: Vec::new() }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ShellScript {
    pub pipelines: Vec<CommandPipeline>,
}

impl ShellScript {
    pub fn new() -> Self {
        Self {
            pipelines: Vec::new(),
        }
    }
}

/// Errors returned by command routing or command validation layers.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ShellCommandError {
    EmptyCommand,
    UnknownCommand,
    InvalidArguments,
}
