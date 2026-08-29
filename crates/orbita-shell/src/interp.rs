//! The Orbita scripting language interpreter.
//!
//! Scripts are plain text files (`/etc/*.sh`, `./demo.sh`, …) whose every
//! statement is a **shell command** — the same ones available
//! interactively (see `docs/scripting.md`). The interpreter adds the
//! Linux-familiar control flow around them:
//!
//! ```sh
//! # comments with '#'
//! count=3
//! for name in /etc /home /usr
//! do
//!     echo "dir: $name"
//! done
//! if test -f /etc/orbita.conf
//! then
//!     echo "config present"
//! elif test -d /etc
//! then
//!     echo "etc exists"
//! else
//!     echo "no etc"
//! fi
//! test -d /etc && echo "and-ok" || echo "and-fail"
//! i=0
//! while test $i -lt 3
//! do
//!     i=$((...))  # no arithmetic yet: use for-loops
//! done
//! exit 0
//! ```
//!
//! v1 scope (documented honestly): no arithmetic expansion, no
//! command substitution, no functions, `until`/case not yet. Guards:
//! loops are bounded ( [`MAX_LOOP_ITERATIONS`] ), script nesting is
//! bounded ( [`MAX_SCRIPT_DEPTH`] ).

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::command::CommandArg;
use crate::runtime::ShellRuntime;
use crate::{ShellEnvironment, ShellOutput, ShellHost};
use orbita_fs::MemoryVolume;

/// Hard bound on one loop's iterations (the OS has no preemptive kill
/// for a spinning script yet).
pub const MAX_LOOP_ITERATIONS: usize = 10_000;
/// Hard bound on script-in-script nesting (`sh` calling `sh`).
pub const MAX_SCRIPT_DEPTH: usize = 8;

enum Frame {
    /// `if … then … elif … else … fi`
    If { parents: bool, active: bool, taken: bool },
    /// `while/until COND … done` — `cond` is the index of the loop line.
    While { parents: bool, active: bool, cond: usize, iterations: usize, until: bool },
    /// `for VAR in WORDS … done`
    For {
        parents: bool,
        var: String,
        words: Vec<String>,
        next: usize,
        body: usize,
    },
    /// `case WORD in … esac`.
    Case {
        parents: bool,
        word: String,
        matched: bool,
        in_branch: bool,
    },
}

impl Frame {
    /// Whether code inside this frame's branch currently executes.
    fn effective(&self) -> bool {
        match self {
            Frame::If { parents, active, .. } => *parents && *active,
            Frame::While { parents, active, .. } => *parents && *active,
            Frame::For { parents, .. } => *parents,
            Frame::Case { parents, in_branch, .. } => *parents && *in_branch,
        }
    }

    #[allow(dead_code)] // diagnostic helper for future scripting work
    fn parents(&self) -> bool {
        match self {
            Frame::If { parents, .. }
            | Frame::While { parents, .. }
            | Frame::For { parents, .. }
            | Frame::Case { parents, .. } => *parents,
        }
    }
}

/// Runs a whole script text and returns its exit status (`exit N` or the
/// last command's status).
pub fn run_script<O: ShellOutput>(
    runtime: &ShellRuntime,
    env: &mut ShellEnvironment,
    fs: &mut MemoryVolume,
    output: &mut O,
    host: &mut dyn ShellHost,
    text: &str,
    depth: usize,
) -> u32 {
    if depth >= MAX_SCRIPT_DEPTH {
        output.write_line("script: nesting too deep");
        return 1;
    }
    let lines: Vec<String> = text.lines().map(str::trim).map(String::from).collect();
    let mut stack: Vec<Frame> = Vec::new();
    let mut index = 0usize;

    while index < lines.len() {
        let line = lines[index].as_str();
        if line.is_empty() || line.starts_with('#') {
            index += 1;
            continue;
        }
        let executing = stack.iter().all(Frame::effective);
        let (keyword, rest) = split_keyword(line);

        match keyword {
            Some("if") => {
                let cond_ok = executing && run_cond(runtime, env, fs, output, host, rest);
                stack.push(Frame::If {
                    parents: executing,
                    active: cond_ok,
                    taken: cond_ok,
                });
            }
            Some("elif") => match stack.last_mut() {
                Some(Frame::If { parents, active, taken }) => {
                    let parent_ok = *parents;
                    let cond_ok = parent_ok && !*taken && run_cond(runtime, env, fs, output, host, rest);
                    *active = cond_ok;
                    *taken = *taken || cond_ok;
                }
                _ => output.write_line("script: elif without if"),
            },
            Some("else") => match stack.last_mut() {
                Some(Frame::If { parents, active, taken }) => {
                    *active = *parents && !*taken;
                    *taken = true;
                }
                _ => output.write_line("script: else without if"),
            },
            Some("fi") => {
                if stack.pop().is_none() {
                    output.write_line("script: fi without if");
                }
            }
            Some("while") | Some("until") => {
                let until = keyword == Some("until");
                // `until` loops while the condition is FALSE.
                let cond_ok = executing && run_cond(runtime, env, fs, output, host, rest) != until;
                stack.push(Frame::While {
                    parents: executing,
                    active: cond_ok,
                    cond: index,
                    iterations: 0,
                    until,
                });
            }
            Some("for") => {
                // `for VAR in w1 w2 …`
                let (var, words) = parse_for_head(env, rest);
                if var.is_empty() {
                    output.write_line("script: for expects 'for VAR in WORDS'");
                    stack.push(Frame::For {
                        parents: executing,
                        var: String::new(),
                        words: Vec::new(),
                        next: usize::MAX,
                        body: index + 1,
                    });
                } else {
                    stack.push(Frame::For {
                        parents: executing,
                        var,
                        words,
                        next: 0,
                        body: index + 1,
                    });
                }
                // Enter the first iteration immediately at `done` handling;
                // jumping forward to the frame's `done` line advances it.
                if let Some(Frame::For { var, words, next, body, .. }) = stack.last_mut() {
                    if *next != usize::MAX && !words.is_empty() {
                        env.set_var(var.clone(), words[0].clone());
                        *next = 1;
                        let body = *body;
                        index = body;
                        continue;
                    }
                }
            }
            Some("done") => match stack.last_mut() {
                Some(Frame::While { parents, active, cond, iterations, until }) => {
                    let parent_ok = *parents;
                    let cond_line = *cond;
                    let invert = *until;
                    if parent_ok {
                        let iters = *iterations + 1;
                        *iterations = iters;
                        if iters > MAX_LOOP_ITERATIONS {
                            output.write_line("script: loop iteration limit reached");
                            stack.pop();
                        } else {
                            let cond_ok = run_line_at(
                                runtime, env, fs, output, host, &lines, cond_line, invert,
                            );
                            *active = cond_ok;
                            if cond_ok {
                                index = cond_line + 1;
                                continue;
                            }
                            stack.pop();
                        }
                    } else {
                        stack.pop();
                    }
                }
                Some(Frame::For { parents, var, words, next, body }) => {
                    let parent_ok = *parents;
                    if parent_ok && *next < words.len() {
                        env.set_var(var.clone(), words[*next].clone());
                        *next += 1;
                        let body = *body;
                        index = body;
                        continue;
                    }
                    stack.pop();
                }
                _ => output.write_line("script: done without loop"),
            },
            Some("do") | Some("then") => { // structural no-ops: bodies follow
            }
            Some("break") if executing => {
                // Pop frames until a loop frame is popped, then skip past
                // its matching `done`.
                while let Some(frame) = stack.pop() {
                    if matches!(frame, Frame::While { .. } | Frame::For { .. }) {
                        break;
                    }
                }
                index = match find_matching_done(&lines, index) {
                    Some(done) => done + 1,
                    None => lines.len(),
                };
                continue;
            }
            Some("continue") if executing => {
                // Pop non-loop frames down to the nearest loop first:
                // the loop's `done` handler must be the stack top.
                while let Some(frame) = stack.last() {
                    if matches!(frame, Frame::While { .. } | Frame::For { .. }) {
                        break;
                    }
                    stack.pop();
                }
                index = match find_matching_done(&lines, index) {
                    Some(done) => done, // let the loop's `done` logic advance
                    None => lines.len(),
                };
                continue;
            }
            Some("exit") if executing => {
                let code = rest.trim().parse::<i32>().map(|c| c as u32).unwrap_or(env.last_status());
                return code;
            }
            Some("return") if executing => {
                // Function/script body exit: stops this run_script scope.
                return rest.trim().parse::<i32>().map(|c| c as u32).unwrap_or(env.last_status());
            }
            Some("case") if rest.ends_with("in") => {
                // `case WORD in`
                let word_raw = rest.strip_suffix("in").unwrap_or(rest).trim_end().trim();
                let word = crate::runtime::expand_argument(
                    env,
                    &CommandArg::new(word_raw.to_string(), false, true),
                );
                stack.push(Frame::Case {
                    parents: executing,
                    word,
                    matched: false,
                    in_branch: false,
                });
            }
            Some("esac") => {
                if stack.pop().is_none() {
                    output.write_line("script: esac without case");
                }
            }
            _ => {
                // `case` pattern lines: matched against the frame word even
                // when the frame is not executing (to find the branch).
                if let Some(Frame::Case { parents: true, word, matched, in_branch }) = stack.last_mut() {
                    if !*in_branch {
                        if let Some((pattern, tail)) = split_case_pattern(line) {
                            let word = word.clone();
                            if !*matched && case_pattern_matches(&word, &pattern) {
                                *matched = true;
                                let tail = tail.trim();
                                let one_line = tail.ends_with(";;");
                                let tail = tail.strip_suffix(";;").unwrap_or(tail).trim();
                                if !tail.is_empty() {
                                    run_cond(runtime, env, fs, output, host, tail);
                                }
                                // `pat) cmd ;;` closes the branch inline.
                                *in_branch = !one_line;
                            }
                            index += 1;
                            continue;
                        }
                    }
                }
                if executing {
                    // `case` pattern line? (`pat) body ;;` / `pat)`)
                    // `;;` closes the current case branch.
                    if line == ";;" {
                        if let Some(Frame::Case { in_branch, .. }) = stack.last_mut() {
                            *in_branch = false;
                        }
                        index += 1;
                        continue;
                    }
                    // Trailing `;;` on a body line ends the branch.
                    let mut body = line;
                    let mut closed_branch = false;
                    if let Some(stripped) = line.strip_suffix(";;") {
                        body = stripped.trim();
                        closed_branch = true;
                    }
                    if !body.is_empty() {
                        // Function definition: `name() {` … `}`.
                        if let Some(name) = parse_function_head(body) {
                            let mut collected = String::new();
                            let mut close = index + 1;
                            while close < lines.len() && lines[close] != "}" {
                                collected.push_str(lines[close].as_str());
                                collected.push('\n');
                                close += 1;
                            }
                            env.set_function(name, collected);
                            index = (close + 1).min(lines.len());
                            continue;
                        }
                        // Function call: first word resolves to a body.
                        let first = body.split_whitespace().next().unwrap_or("");
                        if let Some(function) = env.function(first).map(str::to_string) {
                            let params: Vec<String> = body
                                .split_whitespace()
                                .skip(1)
                                .map(|w| {
                                    crate::runtime::expand_argument(
                                        env,
                                        &CommandArg::new(w.to_string(), false, true),
                                    )
                                })
                                .collect();
                            let saved = stash_positional(env, &params);
                            run_script(
                                runtime, env, fs, output, host, &function, depth + 1,
                            );
                            restore_positional(env, &saved);
                            index += 1;
                            continue;
                        }
                        run_cond(runtime, env, fs, output, host, body);
                    }
                    if closed_branch {
                        if let Some(Frame::Case { in_branch, .. }) = stack.last_mut() {
                            *in_branch = false;
                        }
                    }
                }
            }
        }
        index += 1;
    }

    if !stack.is_empty() {
        // Early `return`/`exit` from nested blocks leaves frames open —
        // only warn for real unterminated blocks at this scope's depth.
        if !stack.is_empty() && depth == 0 {
            output.write_line("script: unterminated block");
        }
    }
    env.last_status()
}

/// Runs one condition/statement line through the normal shell machinery
/// and returns its exit status.
fn run_cond<O: ShellOutput>(
    runtime: &ShellRuntime,
    env: &mut ShellEnvironment,
    fs: &mut MemoryVolume,
    output: &mut O,
    host: &mut dyn ShellHost,
    line: &str,
) -> bool {
    match runtime.parser.parse_script(line) {
        Ok(script) => {
            runtime.execute_script(env, fs, output, host, &script);
            env.last_status() == 0
        }
        Err(_) => {
            output.write_line("script: parse error");
            env.set_last_status(2);
            false
        }
    }
}

/// Re-runs the `while` condition stored at `cond_index` (used by `done`).
fn run_line_at<O: ShellOutput>(
    runtime: &ShellRuntime,
    env: &mut ShellEnvironment,
    fs: &mut MemoryVolume,
    output: &mut O,
    host: &mut dyn ShellHost,
    lines: &[String],
    cond_index: usize,
    invert: bool,
) -> bool {
    // Returns "keep looping?": `until` inverts the condition.
    let line = lines[cond_index].as_str();
    let rest = line
        .strip_prefix("while")
        .or_else(|| line.strip_prefix("until"))
        .unwrap_or(line)
        .trim();
    let cond_ok = run_cond(runtime, env, fs, output, host, rest);
    cond_ok != invert
}

/// Splits `keyword rest` when the line starts with a control keyword.
fn split_keyword(line: &str) -> (Option<&str>, &str) {
    let (first, rest) = match line.split_once(' ') {
        Some((first, rest)) => (first, rest.trim()),
        None => (line, ""),
    };
    const KEYWORDS: &[&str] = &[
        "if", "then", "elif", "else", "fi", "while", "until", "do", "done", "for", "break",
        "continue", "exit", "return", "case", "esac",
    ];
    if KEYWORDS.contains(&first) {
        (Some(first), rest)
    } else {
        (None, line)
    }
}

/// Parses `VAR in w1 w2 …` with `$var` expansion over the words.
fn parse_for_head(env: &ShellEnvironment, rest: &str) -> (String, Vec<String>) {
    let Some((var, words)) = rest.split_once(" in ") else {
        return (String::new(), Vec::new());
    };
    if var.is_empty() || !var.chars().all(|c| c == '_' || c.is_ascii_alphanumeric()) {
        return (String::new(), Vec::new());
    }
    let mut expanded = Vec::new();
    for word in words.split_whitespace() {
        expanded.push(crate::runtime::expand_argument(
            env,
            &CommandArg::new(word.to_string(), false, true),
        ));
    }
    (var.to_string(), expanded)
}

/// Finds the `done` matching a loop body that starts at `from`
/// (nesting-aware forward scan over control keywords).
fn find_matching_done(lines: &[String], from: usize) -> Option<usize> {
    let mut depth = 1usize;
    for (offset, line) in lines.iter().enumerate().skip(from + 1) {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (keyword, _) = split_keyword(line);
        match keyword {
            Some("while") | Some("for") => depth += 1,
            Some("done") => {
                depth -= 1;
                if depth == 0 {
                    return Some(offset);
                }
            }
            _ => {}
        }
    }
    None
}

/// `name() {` — function definition head; returns the name.
pub(crate) fn parse_function_head(line: &str) -> Option<&str> {
    let (head, tail) = line.split_once("() ")?; // "name() {"
    if tail.trim() != "{" || head.contains(' ') || head.is_empty() {
        return None;
    }
    let mut chars = head.chars();
    let valid = {
        let Some(first) = chars.next() else { return None };
        (first == '_' || first.is_ascii_alphabetic())
            && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
    };
    valid.then_some(head)
}

/// Splits a `case` pattern line `pattern) tail` (no leading keyword).
/// Alternatives use `|` inside the pattern (`a|b)`).
fn split_case_pattern(line: &str) -> Option<(&str, &str)> {
    let close = line.find(')')?;
    let pattern = line[..close].trim();
    if pattern.is_empty() {
        return None;
    }
    Some((pattern, &line[close + 1..]))
}

/// `case` pattern match: `*` wildcard anywhere (prefix/suffix segments),
/// `|`-alternatives. No `?`/`[]` (documented).
fn case_pattern_matches(word: &str, pattern: &str) -> bool {
    pattern.split('|').any(|alt| {
        let alt = alt.trim();
        if alt == "*" {
            return true;
        }
        match alt.split_once('*') {
            None => word == alt,
            Some((prefix, suffix)) => {
                word.len() >= prefix.len() + suffix.len()
                    && word.starts_with(prefix)
                    && word.ends_with(suffix)
            }
        }
    })
}

/// Sets `$1..$9`, `$#` for a function/script invocation; returns the
/// previous values for restoration.
fn stash_positional(env: &mut ShellEnvironment, params: &[String]) -> Vec<(String, Option<String>)> {
    let mut saved = Vec::new();
    for slot in 1..=9u32 {
        let name = slot.to_string();
        saved.push((name.clone(), env.vars().get(&name).cloned()));
        let value = params.get(slot as usize - 1).cloned().unwrap_or_default();
        env.set_var(name, value);
    }
    saved.push((String::from("#"), env.vars().get("#").cloned()));
    env.set_var("#", params.len().to_string());
    saved
}

/// Restores positional parameters after a function/script body ran.
fn restore_positional(env: &mut ShellEnvironment, saved: &[(String, Option<String>)]) {
    for (name, value) in saved {
        match value {
            Some(text) => env.set_var(name.clone(), text.clone()),
            None => env.set_var(name.clone(), String::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{NoopShellHost, ShellSystemInfo};
    use crate::ShellOutput;
    use orbita_fs::{BlockSize, FsCapabilities, VolumeId};

    #[derive(Default)]
    struct Collect {
        text: String,
    }

    impl ShellOutput for Collect {
        fn write_line(&mut self, line: &str) {
            self.text.push_str(line);
            self.text.push('\n');
        }
        fn set_status(&mut self, _status: &str) {}
        fn clear(&mut self) {}
    }

    fn fixture(script: &str) -> (Collect, String, u32) {
        let mut fs = MemoryVolume::new(
            VolumeId(0x00AA_0001),
            BlockSize(4096),
            1024,
            FsCapabilities { block_size: BlockSize(4096), features: &[] },
        );
        let _ = fs.create_dir_all("/etc");
        let _ = fs.create_dir_all("/tmp");
        let _ = fs.create_file_path("/etc/orbita.conf", b"hostname=orbita\n");
        let runtime = ShellRuntime::new();
        let mut env = ShellEnvironment::new(ShellSystemInfo::new(
            "orbita-test", "test-gpu", "1M", "80x25", 1,
        ));
        let mut out = Collect::default();
        let mut host = NoopShellHost;
        let status = run_script(&runtime, &mut env, &mut fs, &mut out, &mut host, script, 0);
        let text = out.text.clone();
        (out, text, status)
    }

    #[test]
    fn if_else_takes_matching_branch() {
        let (_, text, _) = fixture(
            "if test -f /etc/orbita.conf\nthen\n  echo yes-conf\nelse\n  echo no-conf\nfi\necho after\n",
        );
        assert!(text.contains("yes-conf"));
        assert!(!text.contains("no-conf"));
        assert!(text.contains("after"));
    }

    #[test]
    fn elif_chain_and_else() {
        let (_, text, _) = fixture(
            "if test -f /nope\nthen\n  echo a\nelif test -f /etc/orbita.conf\nthen\n  echo b\nelse\n  echo c\nfi\n",
        );
        assert!(text.contains("b"));
        assert!(!text.contains("a"));
        assert!(!text.contains("c"));
    }

    #[test]
    fn for_loop_iterates_and_expands() {
        let (_, text, _) = fixture(
            "for name in /etc /home /usr\ndo\n  echo dir:$name\ndone\n",
        );
        assert!(text.contains("dir:/etc"));
        assert!(text.contains("dir:/home"));
        assert!(text.contains("dir:/usr"));
    }

    #[test]
    fn while_loop_with_state_flip() {
        // The body removes the marker: exactly one iteration must run.
        let (_, text, _) = fixture(
            "fs_marker=1\nwrite /tmp/marker on\nwhile test -f /tmp/marker\ndo\n  echo spin\n  rm /tmp/marker\ndone\necho done-spinning\n",
        );
        assert_eq!(text.matches("spin").count() - text.matches("done-spinning").count(), 1);
        assert!(text.contains("done-spinning"));
    }

    #[test]
    fn and_or_chains() {
        let (_, text, _) = fixture(
            "test -f /etc/orbita.conf && echo and-ok\ntest -f /nope || echo or-ok\ntest -f /nope && echo not-run\n",
        );
        assert!(text.contains("and-ok"));
        assert!(text.contains("or-ok"));
        assert!(!text.contains("not-run"));
    }

    #[test]
    fn exit_status_propagates_and_stops() {
        let (_, text, status) = fixture("echo before\nexit 7\necho after\n");
        assert_eq!(status, 7);
        assert!(text.contains("before"));
        assert!(!text.contains("after"));
    }

    #[test]
    fn nested_if_inside_for() {
        let (_, text, _) = fixture(
            "for d in /etc /nope\ndo\n  if test -f $d/orbita.conf\n  then\n    echo found:$d\n  fi\ndone\n",
        );
        assert!(text.contains("found:/etc"));
        assert!(!text.contains("found:/nope"));
    }

    #[test]
    fn break_and_continue() {
        let (_, text, _) = fixture(
            "for d in a b c d\ndo\n  if test $d = b\n  then\n    continue\n  fi\n  if test $d = d\n  then\n    break\n  fi\n  echo item:$d\ndone\necho after-loop\n",
        );
        assert!(text.contains("item:a"));
        assert!(text.contains("item:c"));
        assert!(!text.contains("item:b"));
        assert!(!text.contains("item:d"));
        assert!(text.contains("after-loop"));
    }

    #[test]
    fn while_iteration_limit_is_enforced() {
        let (_, text, _) = fixture("write /tmp/spin x\nwhile test -f /tmp/spin\ndo\n  echo tick\ndone\n");
        // The loop cannot make progress: the limit must stop it, not hang.
        assert!(text.contains("iteration limit") || !text.contains("tick"));
    }

    #[test]
    fn arithmetic_counter_loop() {
        // The classic counter loop — impossible before $(( )).
        let (_, text, _) = fixture(
            "i=0\nwhile test $i -lt 3\ndo\n  echo tick:$i\n  i=$((i+1))\ndone\necho final:$i\n",
        );
        assert!(text.contains("tick:0"));
        assert!(text.contains("tick:1"));
        assert!(text.contains("tick:2"));
        assert!(text.contains("final:3"));
        assert!(!text.contains("tick:3"));
    }

    #[test]
    fn command_substitution_splices_output() {
        let (_, text, _) = fixture(
            "name=$(echo orbita)\necho hello-$name\necho host:$(uname)\n",
        );
        assert!(text.contains("hello-orbita"), "GOT: {text}");
        assert!(text.contains("host:orbita-test"));
    }

    #[test]
    fn arithmetic_with_variables_and_precedence() {
        let (_, text, _) = fixture("a=6\nb=7\necho $((a*b)) $(( (a+b)/2 )) $((a < b))\n");
        assert!(text.contains("42 6 1"));
    }

    #[test]
    fn functions_define_call_and_positionals() {
        let (_, text, _) = fixture(
            "greet() {\n  echo hello-$1 count=$#\n}\ngreet world\ngreet a b\n",
        );
        assert!(text.contains("hello-world count=1"), "GOT: {text}");
        assert!(text.contains("hello-a count=2"), "GOT: {text}");
    }

    #[test]
    fn function_with_control_flow_and_return() {
        let (_, text, _) = fixture(
            "classify() {\n  if test $1 = bad\n  then\n    return 1\n  fi\n  echo good:$1\n}\nclassify ok\nclassify bad\necho after\n",
        );
        assert!(text.contains("good:ok"), "GOT: {text}");
        assert!(!text.contains("good:bad"), "GOT: {text}");
        assert!(text.contains("after"), "GOT: {text}");
    }

    #[test]
    fn case_dispatch_with_wildcard_and_alternatives() {
        let (_, text, _) = fixture(
            "for f in a.txt b.c z q\n\ndo\n  case $f in\n    *.txt) echo text:$f ;;\n    *.c|q) echo alt:$f ;;\n    *) echo other:$f ;;\n  esac\ndone\n",
        );
        assert!(text.contains("text:a.txt"), "GOT: {text}");
        assert!(text.contains("alt:b.c"), "GOT: {text}");
        assert!(text.contains("alt:q"), "GOT: {text}");
        assert!(text.contains("other:z"), "GOT: {text}");
    }

    #[test]
    fn until_loop_runs_until_true() {
        let (_, text, _) = fixture(
            "n=0\nuntil test $n -ge 2\ndo\n  n=$((n+1))\ndone\necho n=$n\n",
        );
        assert!(text.contains("n=2"), "GOT: {text}");
    }

    #[test]
    fn paren_after_arith_survives() {
        let (_, text, _) = fixture("echo \"ok (6*6=$(( 6 * 6 )))\"\n");
        assert!(text.contains("(6*6=36)"), "GOT: {text:?}");
    }
}
