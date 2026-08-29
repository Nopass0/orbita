use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::fmt::Write;

use orbita_core::builtin_apps;
use orbita_fs::{MemoryVolume, VolumeInspector};

use crate::command::{CommandArg, RedirectKind, ShellScript, SimpleCommand};
use crate::{ParseError, ShellParser};

pub trait ShellOutput {
    fn write_line(&mut self, line: &str);
    fn set_status(&mut self, status: &str);
    fn clear(&mut self);
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ShellSystemInfo {
    pub uname: String,
    pub gpu: String,
    pub memory: String,
    pub resolution: String,
    pub logical_cpus: u32,
}

impl ShellSystemInfo {
    pub fn new(
        uname: impl Into<String>,
        gpu: impl Into<String>,
        memory: impl Into<String>,
        resolution: impl Into<String>,
        logical_cpus: u32,
    ) -> Self {
        Self {
            uname: uname.into(),
            gpu: gpu.into(),
            memory: memory.into(),
            resolution: resolution.into(),
            logical_cpus,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ShellEnvironment {
    cwd: String,
    history: Vec<String>,
    vars: BTreeMap<String, String>,
    functions: BTreeMap<String, String>,
    last_status: u32,
    system: ShellSystemInfo,
    active_app: String,
}

impl ShellEnvironment {
    pub fn new(system: ShellSystemInfo) -> Self {
        let mut vars = BTreeMap::new();
        vars.insert(String::from("HOME"), String::from("/home/user"));
        vars.insert(String::from("PWD"), String::from("/"));
        vars.insert(String::from("USER"), String::from("user"));
        vars.insert(String::from("SHELL"), String::from("/bin/orbita-sh"));
        vars.insert(String::from("PATH"), String::from("/usr/bin:/bin:/opt/toolchains/bin"));
        vars.insert(String::from("TERM"), String::from("orbita-framebuffer"));
        vars.insert(String::from("LANG"), String::from("C.UTF-8"));

        Self {
            cwd: String::from("/"),
            history: Vec::new(),
            vars,
            functions: BTreeMap::new(),
            last_status: 0,
            system,
            active_app: String::from("terminal"),
        }
    }

    /// Defines (or replaces) a shell function body (scripting language).
    pub fn set_function(&mut self, name: impl Into<String>, body: impl Into<String>) {
        self.functions.insert(name.into(), body.into());
    }

    /// Looks up a function definition.
    pub fn function(&self, name: &str) -> Option<&str> {
        self.functions.get(name).map(String::as_str)
    }

    pub fn cwd(&self) -> &str {
        self.cwd.as_str()
    }

    pub fn history(&self) -> &[String] {
        &self.history
    }

    pub fn last_status(&self) -> u32 {
        self.last_status
    }

    pub fn set_last_status(&mut self, status: u32) {
        self.last_status = status;
    }

    pub fn set_var(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.vars.insert(name.into(), value.into());
    }

    pub fn vars(&self) -> &BTreeMap<String, String> {
        &self.vars
    }

    pub fn active_app(&self) -> &str {
        self.active_app.as_str()
    }

    pub fn set_active_app(&mut self, id: impl Into<String>) {
        self.active_app = id.into();
    }

    pub fn set_cwd(&mut self, cwd: impl Into<String>) {
        self.cwd = cwd.into();
        self.sync_pwd();
    }

    fn sync_pwd(&mut self) {
        self.vars.insert(String::from("PWD"), self.cwd.clone());
    }
}

/// Kernel-side services the shell can invoke.
///
/// Implemented by the kernel terminal host: native application execution
/// (`run`), the process table (`ps`), and live networking (`ping`).
/// Keeping this as a trait keeps `orbita-shell` a pure `no_std` library
/// with no dependency on kernel internals.
pub trait ShellHost {
    /// Execute an installed native application image. Returns its exit
    /// code and captured stdout lines.
    fn exec_app(&mut self, env: &mut ShellEnvironment, fs: &mut MemoryVolume, path: &str, args: &[String])
        -> Result<(i32, String), String>;
    /// Snapshot of the process table (pid, name, state) for `ps`.
    fn process_rows(&mut self) -> Vec<(u32, String, String)>;
    /// Send one ICMP echo request and return a human-readable result line.
    fn ping(&mut self, target: &str) -> String;
}

/// A [`ShellHost`] used in tests/offline contexts: every service is
/// reported as unavailable instead of silently succeeding.
pub struct NoopShellHost;

impl ShellHost for NoopShellHost {
    fn exec_app(
        &mut self,
        _env: &mut ShellEnvironment,
        _fs: &mut MemoryVolume,
        _path: &str,
        _args: &[String],
    ) -> Result<(i32, String), String> {
        Err(String::from("run: process loader unavailable"))
    }

    fn process_rows(&mut self) -> Vec<(u32, String, String)> {
        Vec::new()
    }

    fn ping(&mut self, _target: &str) -> String {
        String::from("ping: network unreachable")
    }
}

pub struct ShellRuntime {
    pub(crate) parser: ShellParser,
}

impl ShellRuntime {
    pub const fn new() -> Self {
        Self {
            parser: ShellParser::new(),
        }
    }

    pub fn execute_line<O: ShellOutput>(
        &self,
        env: &mut ShellEnvironment,
        fs: &mut MemoryVolume,
        output: &mut O,
        host: &mut dyn ShellHost,
        input: &str,
    ) {
        env.history.push(input.to_string());
        match self.parser.parse_script(input) {
            Ok(script) => self.execute_script(env, fs, output, host, &script),
            Err(error) => {
                output.write_line(&format!("parse error: {:?}", error));
                output.set_status("parse error");
                env.last_status = 2;
            }
        }
    }

    pub fn execute_script_text<O: ShellOutput>(
        &self,
        env: &mut ShellEnvironment,
        fs: &mut MemoryVolume,
        output: &mut O,
        host: &mut dyn ShellHost,
        input: &str,
    ) -> Result<(), ParseError> {
        let script = self.parser.parse_script(input)?;
        self.execute_script(env, fs, output, host, &script);
        Ok(())
    }

    pub(crate) fn execute_script<O: ShellOutput>(
        &self,
        env: &mut ShellEnvironment,
        fs: &mut MemoryVolume,
        output: &mut O,
        host: &mut dyn ShellHost,
        script: &ShellScript,
    ) {
        self.execute_substituted(env, fs, output, host, script, 0);
    }

    /// Depth-carrying executor: pipelines inside `$( … )` substitutions
    /// run through here so nested substitutions stay bounded.
    pub(crate) fn execute_substituted<O: ShellOutput>(
        &self,
        env: &mut ShellEnvironment,
        fs: &mut MemoryVolume,
        output: &mut O,
        host: &mut dyn ShellHost,
        script: &ShellScript,
        depth: u32,
    ) {
        for pipeline in &script.pipelines {
            // `&&` / `||` statement chaining (scripting language).
            match pipeline.connector {
                crate::command::Connector::And if env.last_status != 0 => continue,
                crate::command::Connector::Or if env.last_status == 0 => continue,
                _ => {}
            }
            let mut stdin = String::new();
            let last_index = pipeline.commands.len().saturating_sub(1);
            for (index, command) in pipeline.commands.iter().enumerate() {
                let final_stage = index == last_index;
                let result = self.execute_command_at_depth(env, fs, host, command, &stdin, depth);
                env.last_status = result.status;

                if let Err(message) = apply_redirections(env, fs, command, &result.stdout) {
                    output.write_line(&message);
                    output.set_status("redirection failed");
                    env.last_status = 1;
                    return;
                }

                if final_stage && !has_stdout_redirect(command) {
                    if result.clear_screen {
                        output.clear();
                    }
                    emit_text(output, &result.stdout);
                    output.set_status(result.status_text.as_deref().unwrap_or("ok"));
                }

                stdin = result.stdout;
            }
        }
    }

    /// Depth-tracked command executor: `$( )` substitutions recurse
    /// through here (top level enters with depth 0).
    fn execute_command_at_depth(
        &self,
        env: &mut ShellEnvironment,
        fs: &mut MemoryVolume,
        host: &mut dyn ShellHost,
        command: &SimpleCommand,
        pipeline_input: &str,
        depth: u32,
    ) -> CommandResult {
        let mut scoped = env.vars.clone();
        for assignment in &command.assignments {
            let value =
                expand_arg_with_substitution(self, env, fs, host, &assignment.value, &scoped, depth);
            scoped.insert(assignment.name.clone(), value.clone());
            if command.name.is_none() {
                env.vars.insert(assignment.name.clone(), value);
            }
        }

        let stdin = match read_stdin(env, fs, command, pipeline_input) {
            Ok(stdin) => stdin,
            Err(message) => return CommandResult::err(&message, Some(String::from("redirect failed"))),
        };

        let Some(name) = command.name.as_ref() else {
            env.sync_pwd();
            return CommandResult::ok(String::new(), Some(String::from("ok")));
        };

        let mut args = Vec::new();
        for arg in &command.args {
            args.push(expand_arg_with_substitution(self, env, fs, host, arg, &scoped, depth));
        }

        let result = match name.word.as_str() {
            "help" => help_text(),
            "clear" => CommandResult::clear(),
            "pwd" => CommandResult::ok(env.cwd().to_string(), Some(String::from("ok"))),
            "cd" => command_cd(env, fs, args.first().map(String::as_str).unwrap_or("/")),
            "ls" => command_ls(env, fs, args.first().map(String::as_str).unwrap_or(".")),
            "lsroot" => command_ls(env, fs, "/"),
            "cat" => command_cat(env, fs, &args, &stdin),
            "touch" => command_touch(env, fs, &args),
            "mkdir" => command_mkdir(env, fs, &args),
            "write" => command_write(env, fs, &args, &stdin, false),
            "append" => command_write(env, fs, &args, &stdin, true),
            "rm" => command_rm(env, fs, &args),
            "mv" => command_mv(env, fs, &args),
            "cp" => command_cp(env, fs, &args),
            "df" => command_df(fs),
            "echo" => CommandResult::ok(args.join(" "), Some(String::from("ok"))),
            "history" => CommandResult::ok(env.history().join("\n"), Some(String::from("ok"))),
            "uname" => CommandResult::ok(env.system.uname.clone(), Some(String::from("ok"))),
            "meminfo" => command_meminfo(env, fs),
            "gpuinfo" => CommandResult::ok(env.system.gpu.clone(), Some(String::from("ok"))),
            "apps" => command_apps(fs),
            "services" => command_services(fs),
            "svcrun" => command_config_file(fs, "/run/services/status.toml", "svcrun"),
            "events" => command_config_file(fs, "/run/events.log", "events"),
            "desktop" => command_config_file(fs, "/run/desktop/session.toml", "desktop"),
            "launch" => command_launch(env, &args),
            "toolchains" => command_config_file(fs, "/etc/toolchains.toml", "toolchains"),
            "netcfg" => command_netcfg(fs),
            "ping" => command_ping(host, &args),
            "pkg" | "apt" | "apt-get" | "apk" | "pacman" | "dnf" => {
                command_package_manager(fs, name.word.as_str(), &args)
            }
            "run" => command_run(env, fs, host, &args),
            "ps" => command_ps(host),
            "which" | "type" => command_which(env, fs, &args),
            "motd" => command_cat(env, fs, &[String::from("/etc/motd")], ""),
            "env" => command_env(env),
            "set" | "export" => command_export(env, &args),
            "true" => CommandResult::ok(String::new(), Some(String::from("ok"))),
            "false" => CommandResult::err("false", Some(String::from("false"))),
            "grep" => command_grep(&args, &stdin),
            "wc" => command_wc(&stdin),
            "head" => command_head(&args, &stdin),
            "tail" => command_tail(&args, &stdin),
            "test" => command_test(env, fs, &args),
            "[" => {
                // `[ expr ]` — drop the closing bracket, keep the test.
                let mut expr = args.clone();
                if expr.last().map(String::as_str) == Some("]") {
                    expr.pop();
                }
                command_test(env, fs, &expr)
            }
            "source" | "." | "sh" | "bash" => command_source(self, env, fs, host, &args),
            other => {
                // Script execution by path (like Linux `./script.sh`):
                // any command naming an existing file runs it as a script.
                if let Some(path) = script_by_path(env, fs, other) {
                    return command_source(self, env, fs, host, &[path]);
                }
                if let Some(result) = command_path_dispatch(env, fs, other, &args, &stdin) {
                    result
                } else {
                    CommandResult::err(
                        &format!("unknown command: {other}"),
                        Some(String::from("unknown command")),
                    )
                }
            }
        };

        env.sync_pwd();
        result
    }
}

impl Default for ShellRuntime {
    fn default() -> Self {
        Self::new()
    }
}

struct CommandResult {
    stdout: String,
    status: u32,
    status_text: Option<String>,
    clear_screen: bool,
}

impl CommandResult {
    fn ok(stdout: String, status_text: Option<String>) -> Self {
        Self {
            stdout,
            status: 0,
            status_text,
            clear_screen: false,
        }
    }

    fn err(message: &str, status_text: Option<String>) -> Self {
        Self {
            stdout: message.to_string(),
            status: 1,
            status_text,
            clear_screen: false,
        }
    }

    fn clear() -> Self {
        Self {
            stdout: String::new(),
            status: 0,
            status_text: Some(String::from("cleared")),
            clear_screen: true,
        }
    }
}

fn help_text() -> CommandResult {
    CommandResult::ok(
        [
            "help  pwd  cd [path]  ls [path]  cat [path]  touch <path>",
            "mkdir <path>  write <path> <text>  append <path> <text>  rm <path>",
            "mv <old> <new>  cp <src> <dst>  df  echo <text>  clear",
            "history  uname  meminfo  gpuinfo  apps  services  svcrun  events  desktop",
            "launch <app-id>  netcfg  ping <ip>  which <cmd>  motd  env  export",
            "pkg list | install <name> | remove <name> | info <name>",
            "run <app> [args]   ps                  # native Rust applications",
            "grep <text>  wc  head [n]  tail [n]  source <path>  sh <path>",
            "Supports: ; sequences, | pipes, > >> < redirections, VAR=value, $VAR expansion",
        ]
        .join("\n"),
        Some(String::from("ok")),
    )
}

fn command_cd(env: &mut ShellEnvironment, fs: &mut MemoryVolume, target: &str) -> CommandResult {
    let resolved = resolve_path(env.cwd(), target);
    match fs.list_path(&resolved) {
        Ok(_) => {
            env.set_cwd(resolved);
            CommandResult::ok(String::new(), Some(String::from("ok")))
        }
        Err(err) => CommandResult::err(
            &format!("cd: {:?}", err),
            Some(String::from("cd failed")),
        ),
    }
}

fn command_ls(env: &ShellEnvironment, fs: &mut MemoryVolume, target: &str) -> CommandResult {
    let path = resolve_path(env.cwd(), target);
    match fs.list_path(&path) {
        Ok(listing) => {
            if listing.entries.is_empty() {
                return CommandResult::ok(String::from("(empty)"), Some(String::from("ok")));
            }
            let mut out = String::new();
            for entry in listing.entries {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&entry.name);
                if entry.metadata.is_directory() {
                    out.push('/');
                }
            }
            CommandResult::ok(out, Some(String::from("ok")))
        }
        Err(err) => CommandResult::err(
            &format!("ls: {:?}", err),
            Some(String::from("ls failed")),
        ),
    }
}

fn command_cat(env: &ShellEnvironment, fs: &mut MemoryVolume, args: &[String], stdin: &str) -> CommandResult {
    if args.is_empty() {
        return CommandResult::ok(stdin.to_string(), Some(String::from("ok")));
    }

    let mut out = String::new();
    for path in args {
        let resolved = resolve_path(env.cwd(), path);
        match fs.read_file_path(&resolved) {
            Ok(bytes) => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&String::from_utf8_lossy(&bytes));
            }
            Err(err) => {
                return CommandResult::err(
                    &format!("cat: {:?}", err),
                    Some(String::from("cat failed")),
                );
            }
        }
    }
    if out.is_empty() {
        out.push_str("(empty)");
    }
    CommandResult::ok(out, Some(String::from("ok")))
}

fn command_touch(env: &ShellEnvironment, fs: &mut MemoryVolume, args: &[String]) -> CommandResult {
    let Some(path) = args.first() else {
        return CommandResult::err("touch: missing path", Some(String::from("touch failed")));
    };
    let resolved = resolve_path(env.cwd(), path);
    match fs.create_file_path(&resolved, b"") {
        Ok(()) => CommandResult::ok(format!("created {resolved}"), Some(String::from("ok"))),
        Err(err) => CommandResult::err(
            &format!("touch: {:?}", err),
            Some(String::from("touch failed")),
        ),
    }
}

fn command_mkdir(env: &ShellEnvironment, fs: &mut MemoryVolume, args: &[String]) -> CommandResult {
    let Some(path) = args.first() else {
        return CommandResult::err("mkdir: missing path", Some(String::from("mkdir failed")));
    };
    let resolved = resolve_path(env.cwd(), path);
    match fs.create_dir_all(&resolved) {
        Ok(()) => CommandResult::ok(format!("created {resolved}"), Some(String::from("ok"))),
        Err(err) => CommandResult::err(
            &format!("mkdir: {:?}", err),
            Some(String::from("mkdir failed")),
        ),
    }
}

fn command_write(
    env: &ShellEnvironment,
    fs: &mut MemoryVolume,
    args: &[String],
    stdin: &str,
    append: bool,
) -> CommandResult {
    let Some(path) = args.first() else {
        let message = if append {
            "append: usage append <path> <text>"
        } else {
            "write: usage write <path> <text>"
        };
        let status = if append { "append failed" } else { "write failed" };
        return CommandResult::err(message, Some(String::from(status)));
    };

    let resolved = resolve_path(env.cwd(), path);
    let text = if args.len() > 1 {
        args[1..].join(" ")
    } else {
        stdin.to_string()
    };

    if append {
        return match fs.read_file_path(&resolved) {
            Ok(mut current) => {
                current.extend_from_slice(text.as_bytes());
                match fs.create_file_path(&resolved, &current) {
                    Ok(()) => CommandResult::ok(
                        format!("appended {} bytes to {}", text.len(), resolved),
                        Some(String::from("ok")),
                    ),
                    Err(err) => CommandResult::err(
                        &format!("append: {:?}", err),
                        Some(String::from("append failed")),
                    ),
                }
            }
            Err(err) => CommandResult::err(
                &format!("append: {:?}", err),
                Some(String::from("append failed")),
            ),
        };
    }

    match fs.create_file_path(&resolved, text.as_bytes()) {
        Ok(()) => CommandResult::ok(
            format!("wrote {} bytes to {}", text.len(), resolved),
            Some(String::from("ok")),
        ),
        Err(err) => CommandResult::err(
            &format!("write: {:?}", err),
            Some(String::from("write failed")),
        ),
    }
}

fn command_rm(env: &ShellEnvironment, fs: &mut MemoryVolume, args: &[String]) -> CommandResult {
    let Some(path) = args.first() else {
        return CommandResult::err("rm: missing path", Some(String::from("rm failed")));
    };
    let resolved = resolve_path(env.cwd(), path);
    match fs.remove_path(&resolved) {
        Ok(()) => CommandResult::ok(format!("removed {resolved}"), Some(String::from("ok"))),
        Err(err) => CommandResult::err(
            &format!("rm: {:?}", err),
            Some(String::from("rm failed")),
        ),
    }
}

fn command_mv(env: &ShellEnvironment, fs: &mut MemoryVolume, args: &[String]) -> CommandResult {
    if args.len() != 2 {
        return CommandResult::err("mv: usage mv <old> <new>", Some(String::from("mv failed")));
    }
    let old_path = resolve_path(env.cwd(), &args[0]);
    let new_path = resolve_path(env.cwd(), &args[1]);
    match fs.rename_path(&old_path, &new_path) {
        Ok(()) => CommandResult::ok(
            format!("renamed {} -> {}", old_path, new_path),
            Some(String::from("ok")),
        ),
        Err(err) => CommandResult::err(
            &format!("mv: {:?}", err),
            Some(String::from("mv failed")),
        ),
    }
}

fn command_cp(env: &ShellEnvironment, fs: &mut MemoryVolume, args: &[String]) -> CommandResult {
    if args.len() != 2 {
        return CommandResult::err("cp: usage cp <src> <dst>", Some(String::from("cp failed")));
    }
    let src = resolve_path(env.cwd(), &args[0]);
    let dst = resolve_path(env.cwd(), &args[1]);
    match fs.read_file_path(&src) {
        Ok(bytes) => match fs.create_file_path(&dst, &bytes) {
            Ok(()) => CommandResult::ok(
                format!("copied {} -> {}", src, dst),
                Some(String::from("ok")),
            ),
            Err(err) => CommandResult::err(
                &format!("cp: {:?}", err),
                Some(String::from("cp failed")),
            ),
        },
        Err(err) => CommandResult::err(
            &format!("cp: {:?}", err),
            Some(String::from("cp failed")),
        ),
    }
}

fn command_df(fs: &MemoryVolume) -> CommandResult {
    let stats = fs.volume_stats();
    CommandResult::ok(
        format!(
            "volume total={} free={} used={}%",
            stats.space.total_bytes(),
            stats.space.available_bytes(),
            stats.space.used_percent()
        ),
        Some(String::from("ok")),
    )
}

fn command_meminfo(env: &ShellEnvironment, fs: &MemoryVolume) -> CommandResult {
    let stats = fs.volume_stats();
    CommandResult::ok(
        format!(
            "{}\nresolution={} cpus={}\nfs blocks total={} free={} used={}%",
            env.system.memory,
            env.system.resolution,
            env.system.logical_cpus,
            stats.space.total_blocks,
            stats.space.free_blocks,
            stats.space.used_percent()
        ),
        Some(String::from("ok")),
    )
}

fn command_apps(fs: &mut MemoryVolume) -> CommandResult {
    let manifest = match fs.read_file_path("/system/manifest/apps.toml") {
        Ok(bytes) => String::from_utf8_lossy(&bytes).to_string(),
        Err(err) => {
            return CommandResult::err(
                &format!("apps: {:?}", err),
                Some(String::from("apps failed")),
            )
        }
    };
    let listing = match fs.list_path("/system/apps") {
        Ok(listing) => listing,
        Err(err) => {
            return CommandResult::err(
                &format!("apps: {:?}", err),
                Some(String::from("apps failed")),
            )
        }
    };

    let mut out = String::from("Installed built-in apps:\n");
    for entry in listing.entries {
        let _ = writeln!(&mut out, "- {}", entry.name);
    }
    out.push('\n');
    out.push_str(&manifest);
    CommandResult::ok(out, Some(String::from("ok")))
}

fn command_services(fs: &mut MemoryVolume) -> CommandResult {
    let manifest = match fs.read_file_path("/system/manifest/services.toml") {
        Ok(bytes) => String::from_utf8_lossy(&bytes).to_string(),
        Err(err) => {
            return CommandResult::err(
                &format!("services: {:?}", err),
                Some(String::from("services failed")),
            )
        }
    };
    let listing = match fs.list_path("/system/services") {
        Ok(listing) => listing,
        Err(err) => {
            return CommandResult::err(
                &format!("services: {:?}", err),
                Some(String::from("services failed")),
            )
        }
    };

    let mut out = String::from("Installed services:\n");
    for entry in listing.entries {
        let _ = writeln!(&mut out, "- {}", entry.name);
    }
    out.push('\n');
    out.push_str(&manifest);
    CommandResult::ok(out, Some(String::from("ok")))
}

fn command_launch(env: &mut ShellEnvironment, args: &[String]) -> CommandResult {
    let Some(id) = args.first() else {
        return CommandResult::err("launch: missing app id", Some(String::from("launch failed")));
    };
    if !builtin_apps().iter().any(|app| app.id == id) {
        return CommandResult::err(
            &format!("launch: unknown app id `{id}`"),
            Some(String::from("launch failed")),
        );
    }
    env.set_active_app(id.clone());
    CommandResult::ok(format!("active app -> {id}"), Some(String::from("ok")))
}

fn command_config_file(fs: &mut MemoryVolume, path: &str, label: &str) -> CommandResult {
    match fs.read_file_path(path) {
        Ok(bytes) => CommandResult::ok(
            format!("{label}:\n{}", String::from_utf8_lossy(&bytes)),
            Some(String::from("ok")),
        ),
        Err(err) => CommandResult::err(
            &format!("{label}: {:?}", err),
            Some(format!("{label} failed")),
        ),
    }
}

fn command_env(env: &ShellEnvironment) -> CommandResult {
    let mut out = String::new();
    for (name, value) in env.vars() {
        if !out.is_empty() {
            out.push('\n');
        }
        let _ = write!(&mut out, "{name}={value}");
    }
    CommandResult::ok(out, Some(String::from("ok")))
}

fn command_export(env: &mut ShellEnvironment, args: &[String]) -> CommandResult {
    if args.is_empty() {
        return command_env(env);
    }

    for arg in args {
        match arg.split_once('=') {
            Some((name, value)) if is_identifier(name) => {
                env.vars.insert(name.to_string(), value.to_string());
            }
            _ => {
                return CommandResult::err(
                    "export: expected NAME=value",
                    Some(String::from("export failed")),
                );
            }
        }
    }
    CommandResult::ok(String::new(), Some(String::from("ok")))
}

fn command_grep(args: &[String], stdin: &str) -> CommandResult {
    let Some(needle) = args.first() else {
        return CommandResult::err("grep: missing pattern", Some(String::from("grep failed")));
    };
    let matches: Vec<&str> = stdin.lines().filter(|line| line.contains(needle)).collect();
    CommandResult::ok(matches.join("\n"), Some(String::from("ok")))
}

fn command_wc(stdin: &str) -> CommandResult {
    let lines = stdin.lines().count();
    let words = stdin.split_whitespace().count();
    let bytes = stdin.len();
    CommandResult::ok(format!("{lines} {words} {bytes}"), Some(String::from("ok")))
}

fn command_head(args: &[String], stdin: &str) -> CommandResult {
    let count = args
        .first()
        .and_then(|arg| arg.parse::<usize>().ok())
        .unwrap_or(10);
    let text = stdin.lines().take(count).collect::<Vec<_>>().join("\n");
    CommandResult::ok(text, Some(String::from("ok")))
}

fn command_tail(args: &[String], stdin: &str) -> CommandResult {
    let count = args
        .first()
        .and_then(|arg| arg.parse::<usize>().ok())
        .unwrap_or(10);
    let lines: Vec<&str> = stdin.lines().collect();
    let start = lines.len().saturating_sub(count);
    CommandResult::ok(lines[start..].join("\n"), Some(String::from("ok")))
}

fn command_source(
    runtime: &ShellRuntime,
    env: &mut ShellEnvironment,
    fs: &mut MemoryVolume,
    host: &mut dyn ShellHost,
    args: &[String],
) -> CommandResult {    let Some(path) = args.first() else {
        return CommandResult::err("source: missing path", Some(String::from("source failed")));
    };
    let resolved = resolve_path(env.cwd(), path);
    match fs.read_file_path(&resolved) {
        Ok(bytes) => {
            let script = String::from_utf8_lossy(&bytes).to_string();
            let mut buffer = CapturedOutput::default();
            let status = crate::interp::run_script(runtime, env, fs, &mut buffer, host, &script, 0);
            CommandResult {
                stdout: buffer.text,
                status,
                status_text: Some(if buffer.failed || status != 0 {
                    String::from("script failed")
                } else {
                    String::from("ok")
                }),
                clear_screen: buffer.clear,
            }
        }
        Err(err) => CommandResult::err(
            &format!("source: {:?}", err),
            Some(String::from("source failed")),
        ),
    }
}

/// `test` / `[ … ]` — the scripting condition primitive.
/// Supported: `-z s`, `-n s`, `a = b`, `a != b`, `-f p`, `-d p`,
/// `-eq -ne -lt -le -gt -ge` (integer), `! expr` (negation).
fn command_test(
    env: &ShellEnvironment,
    fs: &mut MemoryVolume,
    args: &[String],
) -> CommandResult {
    let ok = eval_test(env, fs, args);
    CommandResult {
        stdout: String::new(),
        status: if ok { 0 } else { 1 },
        status_text: Some(String::from(if ok { "true" } else { "false" })),
        clear_screen: false,
    }
}

/// Evaluates a `test` expression (host-testable core).
pub(crate) fn eval_test(env: &ShellEnvironment, fs: &mut MemoryVolume, args: &[String]) -> bool {
    if let Some((first, rest)) = args.split_first() {
        if first == "!" {
            return !eval_test(env, fs, rest);
        }
    }
    match args.len() {
        0 => false,
        1 => args[0] != "",
        2 => match args[0].as_str() {
            "-z" => args[1].is_empty(),
            "-n" => !args[1].is_empty(),
            "-f" => {
                let path = resolve_path(env.cwd(), &args[1]);
                matches!(fs.read_file_path(&path), Ok(_))
            }
            "-d" => {
                let path = resolve_path(env.cwd(), &args[1]);
                fs.list_path(&path).is_ok()
            }
            _ => false,
        },
        3 => {
            let (lhs, op, rhs) = (&args[0], args[1].as_str(), &args[2]);
            match op {
                "=" | "==" => lhs == rhs,
                "!=" => lhs != rhs,
                "-eq" | "-ne" | "-lt" | "-le" | "-gt" | "-ge" => {
                    let (Some(a), Some(b)) = (lhs.parse::<i64>().ok(), rhs.parse::<i64>().ok())
                    else {
                        return false;
                    };
                    match op {
                        "-eq" => a == b,
                        "-ne" => a != b,
                        "-lt" => a < b,
                        "-le" => a <= b,
                        "-gt" => a > b,
                        _ => a >= b,
                    }
                }
                _ => false,
            }
        }
        _ => false,
    }
}

/// If `name` names an existing file (relative, `./x.sh` or absolute),
/// return the resolved path — scripts run like Linux `./script.sh`.
pub(crate) fn script_by_path(env: &ShellEnvironment, fs: &mut MemoryVolume, name: &str) -> Option<String> {
    if !name.starts_with("./") && !name.starts_with('/') && !name.starts_with("../") {
        return None;
    }
    let path = resolve_path(env.cwd(), name);
    fs.read_file_path(&path).is_ok().then_some(path)
}

fn command_which(env: &ShellEnvironment, fs: &mut MemoryVolume, args: &[String]) -> CommandResult {
    if args.is_empty() {
        return CommandResult::err("which: missing command name", Some(String::from("which failed")));
    }

    let mut out = Vec::new();
    let mut missing = false;
    for arg in args {
        if builtin_command_names().contains(&arg.as_str()) {
            out.push(format!("{}: builtin", arg));
            continue;
        }

        let wrapper = format!("/usr/bin/{}", arg);
        if fs.read_file_path(wrapper.as_str()).is_ok() {
            out.push(wrapper);
            continue;
        }

        let resolved = resolve_path(env.cwd(), arg);
        if fs.read_file_path(resolved.as_str()).is_ok() {
            out.push(resolved);
            continue;
        }

        out.push(format!("{}: not found", arg));
        missing = true;
    }

    if missing {
        CommandResult::err(&out.join("\n"), Some(String::from("which failed")))
    } else {
        CommandResult::ok(out.join("\n"), Some(String::from("ok")))
    }
}

fn command_path_dispatch(
    _env: &ShellEnvironment,
    _fs: &mut MemoryVolume,
    command: &str,
    _args: &[String],
    _stdin: &str,
) -> Option<CommandResult> {
    // Real native application execution is provided by the kernel process
    // loader (`run <name>`); the shell itself no longer fakes interpreters.
    let _ = command;
    None
}

/// Names of every builtin command, used by `which`/`type`.
fn builtin_command_names() -> &'static [&'static str] {
    &[
        "help", "clear", "pwd", "cd", "ls", "lsroot", "cat", "touch", "mkdir", "write", "append",
        "rm", "mv", "cp", "df", "echo", "history", "uname", "meminfo", "gpuinfo", "apps",
        "services", "svcrun", "events", "desktop", "launch", "toolchains", "netcfg", "ping",
        "pkg", "apt", "apt-get", "apk", "pacman", "dnf", "run", "ps", "which", "type", "motd",
        "env", "set", "export", "true", "false", "grep", "wc", "head", "tail", "source", "sh",
        "bash",
    ]
}

// ---------------------------------------------------------------------------
// Package manager (real): operates on the delivery directory `/pkg`
// (`.orbpkg` bundles staged by the host build) and the application
// directory `/apps` inside the persistent volume.
// ---------------------------------------------------------------------------

const PKG_DELIVERY_DIR: &str = "/pkg";
const PKG_APPS_DIR: &str = "/apps";
const PKG_DB_INSTALLED: &str = "/var/lib/orbita/pkg/installed.txt";

/// Strip `.orbpkg` / `.orbexec` suffixes from a file name.
fn package_stem(name: &str) -> String {
    let mut stem = name;
    for suffix in [".orbpkg", ".orbexec"] {
        if let Some(base) = stem.strip_suffix(suffix) {
            stem = base;
        }
    }
    stem.to_string()
}

fn command_package_manager(fs: &mut MemoryVolume, manager: &str, args: &[String]) -> CommandResult {
    // Normalize the different front-end verbs onto one action set.
    let action = match (manager, args.first().map(String::as_str)) {
        (_, Some("install")) | (_, Some("add")) | ("pacman", Some("-S")) => "install",
        (_, Some("remove")) | (_, Some("uninstall")) | ("pacman", Some("-R")) => "remove",
        (_, Some("list")) | ("pacman", Some("-Q")) | (_, Some("ls")) => "list",
        (_, Some("info")) | (_, Some("status")) | (_, Some("show")) => "info",
        (_, Some("update")) | (_, Some("upgrade")) => "update",
        _ => {
            let usage = format!("{manager}: usage {manager} <list|install|remove|info|update> [name]");
            return CommandResult::err(&usage, Some(String::from("pkg usage")));
        }
    };
    let rest: Vec<String> = args.iter().skip(1).cloned().collect();

    match action {
        "list" => {
            let mut lines = Vec::from([String::from("packages in /pkg (delivery):")]);
            match fs.list_path(PKG_DELIVERY_DIR) {
                Ok(listing) => {
                    if listing.entries.is_empty() {
                        lines.push(String::from("  (empty - stage .orbpkg bundles with `dm pkgbuild`)"));
                    }
                    for entry in listing.entries {
                        let installed = is_installed(fs, &entry.name);
                        lines.push(format!("  {}{}", entry.name, if installed { " [installed]" } else { "" }));
                    }
                }
                Err(err) => {
                    lines.push(format!("  /pkg unavailable: {:?}", err));
                }
            }
            lines.push(String::from("installed apps in /apps:"));
            if let Ok(listing) = fs.list_path(PKG_APPS_DIR) {
                for entry in listing.entries {
                    lines.push(format!("  {}", entry.name));
                }
            }
            CommandResult::ok(lines.join("\n"), Some(String::from("ok")))
        }
        "install" => {
            if rest.is_empty() {
                return CommandResult::err(
                    "pkg install: missing package name",
                    Some(String::from("pkg failed")),
                );
            }
            let mut lines = Vec::new();
            for name in &rest {
                lines.push(pkg_install_one(fs, name));
            }
            CommandResult::ok(lines.join("\n"), Some(String::from("ok")))
        }
        "remove" => {
            if rest.is_empty() {
                return CommandResult::err(
                    "pkg remove: missing package name",
                    Some(String::from("pkg failed")),
                );
            }
            let mut lines = Vec::new();
            for name in &rest {
                lines.push(pkg_remove_one(fs, name));
            }
            CommandResult::ok(lines.join("\n"), Some(String::from("ok")))
        }
        "info" => {
            let Some(name) = rest.first() else {
                return CommandResult::err("pkg info: missing name", Some(String::from("pkg failed")));
            };
            let stem = package_stem(name);
            let bundle = format!("{PKG_DELIVERY_DIR}/{stem}.orbpkg");
            match fs.read_file_path(&bundle) {
                Ok(bytes) => {
                    let mut lines = vec![format!("{stem}: bundle {} bytes", bytes.len())];
                    if let Some(line) = manifest_lookup_line(&bytes, "version") {
                        lines.push(format!("version={}", line));
                    }
                    if let Some(line) = manifest_lookup_line(&bytes, "description") {
                        lines.push(format!("description={}", line));
                    }
                    lines.push(format!("installed={}", is_installed(fs, &format!("{stem}.orbpkg"))));
                    CommandResult::ok(lines.join("\n"), Some(String::from("ok")))
                }
                Err(_) => CommandResult::err(
                    &format!("pkg info: `{stem}` not found in /pkg"),
                    Some(String::from("pkg failed")),
                ),
            }
        }
        _ => {
            // update: delivery dir is rebuilt by the host; nothing to refresh here.
            CommandResult::ok(
                String::from("package delivery /pkg is rebuilt on every host build (dm build)"),
                Some(String::from("ok")),
            )
        }
    }
}

/// Extract a `key=value` manifest line from an `.orbpkg` bundle.
fn manifest_lookup_line(bytes: &[u8], key: &str) -> Option<String> {
    let text = core::str::from_utf8(bytes).ok()?;
    for line in text.lines() {
        if let Some((name, value)) = line.split_once('=') {
            if name.trim() == key {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

fn is_installed(fs: &mut MemoryVolume, bundle_name: &str) -> bool {
    let stem = package_stem(bundle_name);
    fs.read_file_path(PKG_DB_INSTALLED)
        .map(|bytes| {
            core::str::from_utf8(&bytes)
                .map(|text| text.lines().any(|line| line.trim() == stem))
                .unwrap_or(false)
        })
        .unwrap_or(false)
}

fn pkg_install_one(fs: &mut MemoryVolume, name: &str) -> String {
    let stem = package_stem(name);
    let bundle = format!("{PKG_DELIVERY_DIR}/{stem}.orbpkg");
    let bytes = match fs.read_file_path(&bundle) {
        Ok(bytes) => bytes,
        Err(err) => return format!("pkg install: `{stem}` not in /pkg ({:?})", err),
    };

    // The bundle payload is an ORBEXEC image: name=value manifest lines
    // followed by the binary. Copy it into /apps as an installable image.
    let target = format!("{PKG_APPS_DIR}/{stem}.orbexec");
    if let Err(err) = fs.create_file_path(&target, &bytes) {
        return format!("pkg install: write {} failed ({:?})", target, err);
    }

    // Record in the installed database.
    let mut installed = fs.read_file_path(PKG_DB_INSTALLED).unwrap_or_default();
    if !installed.is_empty() {
        installed.push(b'\n');
    }
    installed.extend_from_slice(stem.as_bytes());
    let _ = fs.create_file_path(PKG_DB_INSTALLED, &installed);
    format!("pkg: installed `{stem}` -> {}", target)
}

fn pkg_remove_one(fs: &mut MemoryVolume, name: &str) -> String {
    let stem = package_stem(name);
    let target = format!("{PKG_APPS_DIR}/{stem}.orbexec");
    match fs.remove_path(&target) {
        Ok(()) => {
            let installed = fs.read_file_path(PKG_DB_INSTALLED).unwrap_or_default();
            let text = String::from_utf8_lossy(&installed).to_string();
            let kept: Vec<&str> = text.lines().filter(|line| line.trim() != stem).collect();
            let _ = fs.create_file_path(PKG_DB_INSTALLED, kept.join("\n").as_bytes());
            format!("pkg: removed `{stem}`")
        }
        Err(err) => format!("pkg remove: `{stem}` not installed ({:?})", err),
    }
}

// ---------------------------------------------------------------------------
// Native applications (`run`, `ps`) and live networking (`ping`, `netcfg`).
// ---------------------------------------------------------------------------

fn command_run(
    env: &mut ShellEnvironment,
    fs: &mut MemoryVolume,
    host: &mut dyn ShellHost,
    args: &[String],
) -> CommandResult {
    let Some(name) = args.first() else {
        return CommandResult::err("run: usage run <app> [args]", Some(String::from("run failed")));
    };
    let stem = package_stem(name);
    let direct = resolve_path(env.cwd(), name);
    let candidates = [
        direct.clone(),
        format!("{PKG_APPS_DIR}/{stem}.orbexec"),
        format!("/bin/{}.orbexec", stem),
    ];
    for candidate in &candidates {
        if fs.read_file_path(candidate).is_ok() {
            return match host.exec_app(env, fs, candidate, &args[1..]) {
                Ok((code, app_stdout)) => {
                    let mut stdout = app_stdout;
                    if !stdout.is_empty() {
                        stdout.push('\n');
                    }
                    stdout.push_str(&format!("{} exited with code {}", stem, code));
                    CommandResult {
                        stdout,
                        status: code.unsigned_abs(),
                        status_text: Some(String::from("ok")),
                        clear_screen: false,
                    }
                }
                Err(message) => CommandResult::err(&message, Some(String::from("run failed"))),
            };
        }
    }
    CommandResult::err(
        &format!("run: application `{stem}` not found (pkg install it first)"),
        Some(String::from("run failed")),
    )
}

fn command_ps(host: &mut dyn ShellHost) -> CommandResult {
    let rows = host.process_rows();
    let mut lines = Vec::from([String::from("PID  NAME                 STATE")]);
    if rows.is_empty() {
        lines.push(String::from("(no processes)"));
    }
    for (pid, name, state) in rows {
        lines.push(format!("{:<4} {:<20} {}", pid, name, state));
    }
    CommandResult::ok(lines.join("\n"), Some(String::from("ok")))
}

fn command_ping(host: &mut dyn ShellHost, args: &[String]) -> CommandResult {
    let Some(target) = args.first() else {
        return CommandResult::err("ping: usage ping <ip>", Some(String::from("ping failed")));
    };
    let line = host.ping(target);
    let failed = line.contains("unreachable") || line.contains("failed");
    if failed {
        CommandResult::err(&line, Some(String::from("ping failed")))
    } else {
        CommandResult::ok(line, Some(String::from("ok")))
    }
}

fn command_netcfg(fs: &mut MemoryVolume) -> CommandResult {
    let mut out = String::new();
    for path in ["/etc/network.toml", "/run/network/status.toml"] {
        if let Ok(bytes) = fs.read_file_path(path) {
            let _ = writeln!(&mut out, "{}:", path);
            let _ = writeln!(&mut out, "{}", String::from_utf8_lossy(&bytes));
        }
    }
    if out.is_empty() {
        out.push_str("netcfg: no network configuration files");
    }
    CommandResult::ok(out, Some(String::from("ok")))
}


#[derive(Default)]
struct CapturedOutput {
    text: String,
    clear: bool,
    failed: bool,
}

impl ShellOutput for CapturedOutput {
    fn write_line(&mut self, line: &str) {
        if !self.text.is_empty() {
            self.text.push('\n');
        }
        self.text.push_str(line);
    }

    fn set_status(&mut self, status: &str) {
        self.failed = status.contains("failed") || status.contains("error") || status == "false";
    }

    fn clear(&mut self) {
        self.clear = true;
        self.text.clear();
    }
}

fn has_stdout_redirect(command: &SimpleCommand) -> bool {
    command
        .redirects
        .iter()
        .any(|redirect| matches!(redirect.kind, RedirectKind::Output | RedirectKind::Append))
}

fn apply_redirections(
    env: &ShellEnvironment,
    fs: &mut MemoryVolume,
    command: &SimpleCommand,
    stdout: &str,
) -> Result<(), String> {
    for redirect in &command.redirects {
        let path = resolve_path(env.cwd(), &expand_argument(env, &redirect.target));
        match redirect.kind {
            RedirectKind::Input => {}
            RedirectKind::Output => {
                fs.create_file_path(&path, stdout.as_bytes())
                    .map_err(|err| format!("redirect > {:?}: {:?}", path, err))?;
            }
            RedirectKind::Append => {
                let mut bytes = fs.read_file_path(&path).unwrap_or_default();
                bytes.extend_from_slice(stdout.as_bytes());
                fs.create_file_path(&path, &bytes)
                    .map_err(|err| format!("redirect >> {:?}: {:?}", path, err))?;
            }
        }
    }
    Ok(())
}

fn read_stdin(
    env: &ShellEnvironment,
    fs: &mut MemoryVolume,
    command: &SimpleCommand,
    pipeline_input: &str,
) -> Result<String, String> {
    let mut input = pipeline_input.to_string();
    for redirect in &command.redirects {
        if redirect.kind == RedirectKind::Input {
            let path = resolve_path(env.cwd(), &expand_argument(env, &redirect.target));
            let bytes = fs
                .read_file_path(&path)
                .map_err(|err| format!("redirect < {:?}: {:?}", path, err))?;
            input = String::from_utf8_lossy(&bytes).to_string();
        }
    }
    Ok(input)
}

fn emit_text<O: ShellOutput>(output: &mut O, text: &str) {
    if text.is_empty() {
        return;
    }
    for line in text.lines() {
        output.write_line(line);
    }
}

pub(crate) fn expand_argument(env: &ShellEnvironment, arg: &CommandArg) -> String {
    expand_argument_from(env, env.vars(), arg)
}

fn expand_argument_from(
    env: &ShellEnvironment,
    vars: &BTreeMap<String, String>,
    arg: &CommandArg,
) -> String {
    if !arg.expand {
        return arg.word.clone();
    }

    let chars: Vec<char> = arg.word.chars().collect();
    let mut out = String::new();
    let mut index = 0usize;

    while index < chars.len() {
        if chars[index] != '$' {
            out.push(chars[index]);
            index += 1;
            continue;
        }

        // Arithmetic expansion: $(( expr )) — nesting-aware scan.
        if index + 2 < chars.len() && chars[index + 1] == '(' && chars[index + 2] == '(' {
            if let Some(end) = find_arith_end(&chars, index + 3) {
                let expr: String = chars[index + 3..end].iter().collect();
                let vars = vars.clone();
                let value = crate::arith::eval(&expr, |name| {
                    resolve_var(env, &vars, name).trim().parse::<i64>().ok()
                });
                match value {
                    Some(number) => out.push_str(&number.to_string()),
                    // Division by zero / syntax error: keep the text as-is
                    // so the failure is visible in the output.
                    None => out.push_str(&chars[index..end + 2].iter().collect::<String>()),
                }
                // `end` points at the first `)` of the closing pair: skip
                // exactly the two parens.
                index = end + 2;
                continue;
            }
        }

        if index + 1 < chars.len() && chars[index + 1] == '{' {
            let mut end = index + 2;
            while end < chars.len() && chars[end] != '}' {
                end += 1;
            }
            if end < chars.len() {
                let name: String = chars[index + 2..end].iter().collect();
                out.push_str(resolve_var(env, vars, &name).as_str());
                index = end + 1;
                continue;
            }
        }

        // Single-character special parameters: `$?`, `$#`.
        if index + 1 < chars.len() && (chars[index + 1] == '?' || chars[index + 1] == '#') {
            let name: String = chars[index + 1..index + 2].iter().collect();
            out.push_str(resolve_var(env, vars, &name).as_str());
            index += 2;
            continue;
        }
        let mut end = index + 1;
        while end < chars.len()
            && (chars[end] == '_' || chars[end].is_ascii_alphanumeric())
        {
            end += 1;
        }
        if end == index + 1 {
            out.push('$');
            index += 1;
            continue;
        }
        let name: String = chars[index + 1..end].iter().collect();
        out.push_str(resolve_var(env, vars, &name).as_str());
        index = end;
    }

    out
}

fn resolve_var(env: &ShellEnvironment, vars: &BTreeMap<String, String>, name: &str) -> String {
    if name == "?" {
        return env.last_status.to_string();
    }
    vars.get(name).cloned().unwrap_or_default()
}

/// Finds the `))` closing a `$((` opened before `start` (paren-nesting
/// aware). Returns the index of the first `)` of the closing pair.
fn find_arith_end(chars: &[char], start: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut index = start;
    while index < chars.len() {
        match chars[index] {
            '(' => depth += 1,
            ')' => {
                if depth == 0 {
                    // The first `)` of the closing `))`.
                    return if index + 1 < chars.len() && chars[index + 1] == ')' {
                        Some(index)
                    } else {
                        None
                    };
                }
                depth -= 1;
            }
            _ => {}
        }
        index += 1;
    }
    None
}

/// Command-substitution depth bound (`$( $( $( … ) ) )` chains).
const MAX_SUBST_DEPTH: u32 = 4;

/// Expands one argument: `$(cmd)` substitutions run the command and splice
/// its (newline-trimmed) stdout, then `$VAR` / `$(( ))` expand as usual.
fn expand_arg_with_substitution(
    runtime: &ShellRuntime,
    env: &mut ShellEnvironment,
    fs: &mut MemoryVolume,
    host: &mut dyn ShellHost,
    arg: &CommandArg,
    scoped: &BTreeMap<String, String>,
    depth: u32,
) -> String {
    if !arg.expand || !arg.word.contains("$(") || arg.word.contains("$((") {
        // No plain substitution; `$(( ))`-only words still need arithmetic.
        return expand_argument_from(env, scoped, arg);
    }
    if depth >= MAX_SUBST_DEPTH {
        return format!("substitution too deep: {}", arg.word);
    }

    let chars: Vec<char> = arg.word.chars().collect();
    let mut out = String::new();
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] != '$' {
            out.push(chars[index]);
            index += 1;
            continue;
        }
        // Skip arithmetic: handled by expand_argument_from afterwards.
        if index + 2 < chars.len() && chars[index + 1] == '(' && chars[index + 2] == '(' {
            out.push('$');
            index += 1;
            continue;
        }
        if index + 1 >= chars.len() || chars[index + 1] != '(' {
            out.push(chars[index]);
            index += 1;
            continue;
        }
        // Find the balanced `)` for this `$(`.
        let Some(end) = (|| {
            let mut depth_paren = 1i32;
            let mut at = index + 2;
            while at < chars.len() {
                match chars[at] {
                    '(' => depth_paren += 1,
                    ')' => {
                        depth_paren -= 1;
                        if depth_paren == 0 {
                            return Some(at);
                        }
                    }
                    _ => {}
                }
                at += 1;
            }
            None
        })() else {
            out.push(chars[index]);
            index += 1;
            continue;
        };
        let inner: String = chars[index + 2..end].iter().collect();
        let mut captured = CapturedOutput::default();
        if let Ok(script) = runtime.parser.parse_script(&inner) {
            runtime.execute_substituted(env, fs, &mut captured, host, &script, depth + 1);
            let text = captured.text.trim_end_matches('\n');
            // Multi-line output collapses to spaces (sh semantics).
            out.push_str(&text.replace('\n', " "));
        } else {
            out.push_str(&inner);
        }
        index = end + 1;
    }

    // Remaining `$VAR` / `$(( ))` in the spliced text.
    expand_argument_from(env, scoped, &CommandArg::new(out, arg.quoted, true))
}

fn resolve_path(cwd: &str, input: &str) -> String {
    if input.is_empty() || input == "." {
        return String::from(cwd);
    }
    if input.starts_with('/') {
        return normalize_path(input);
    }
    if cwd == "/" {
        normalize_path(&format!("/{input}"))
    } else {
        normalize_path(&format!("{cwd}/{input}"))
    }
}

fn normalize_path(path: &str) -> String {
    let mut stack: Vec<&str> = Vec::new();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                let _ = stack.pop();
            }
            other => stack.push(other),
        }
    }
    if stack.is_empty() {
        String::from("/")
    } else {
        let mut normalized = String::from("/");
        for (index, part) in stack.iter().enumerate() {
            if index > 0 {
                normalized.push('/');
            }
            normalized.push_str(part);
        }
        normalized
    }
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}
