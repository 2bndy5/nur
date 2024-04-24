use crate::commands::Nur;
use crate::errors::{NurError, NurResult};
use crate::names::NUR_NAME;
use nu_engine::{get_full_help, CallExt};
use nu_parser::escape_for_script_arg;
use nu_parser::parse;
use nu_protocol::report_error;
use nu_protocol::{
    ast::Expr,
    engine::{Command, EngineState, Stack, StateWorkingSet},
    ShellError,
};
use nu_utils::stdout_write_all_and_flush;

pub(crate) fn is_safe_taskname(name: &str) -> bool {
    // This is basically similar to string_should_be_quoted
    // in nushell/crates/nu-parser/src/deparse.rs:1,
    // BUT may change as the requirements are different.
    // Also I added "#" and "^", as seem in
    // nushell/crates/nu-parser/src/parse_keywords.rs:175
    !name.starts_with('$')
        && !(name.chars().any(|c| {
            c == ' '
                || c == '('
                || c == '\''
                || c == '`'
                || c == '"'
                || c == '\\'
                || c == '#'
                || c == '^'
        }))
}

pub(crate) fn gather_commandline_args(
    args: Vec<String>,
) -> NurResult<(Vec<String>, bool, Vec<String>)> {
    let mut args_to_nur = Vec::from([String::from(NUR_NAME)]);
    let mut task_call = Vec::from([String::from(NUR_NAME)]);
    let mut has_task_call = false;
    let mut args_iter = args.iter();

    args_iter.next(); // Ignore own name
    #[allow(clippy::while_let_on_iterator)]
    while let Some(arg) = args_iter.next() {
        if !arg.starts_with('-') {
            // At least first non nur argument must be safe
            if !is_safe_taskname(arg) {
                eprintln!("{}", arg);
                return Err(NurError::InvalidTaskName(arg.clone()));
            }

            // Register task name and switch to task call parsing
            has_task_call = true;
            task_call.push(arg.clone());
            break;
        }

        // let flag_value = match arg.as_ref() {
        //     // "--some-file" => args.next().map(|a| escape_quote_string(&a)),
        //     _ => None,
        // };

        args_to_nur.push(arg.clone());

        // if let Some(flag_value) = flag_value {
        //     args_to_nur.push(flag_value);
        // }
    }

    if has_task_call {
        // Consume remaining elements in iterator
        #[allow(clippy::while_let_on_iterator)]
        while let Some(arg) = args_iter.next() {
            task_call.push(escape_for_script_arg(arg));
        }
    } else {
        // Also remove "nur" from task_call
        task_call.clear();
    }

    Ok((args_to_nur, has_task_call, task_call))
}

pub(crate) fn parse_commandline_args(
    commandline_args: &str,
    engine_state: &mut EngineState,
) -> Result<NurArgs, ShellError> {
    let (block, delta) = {
        let mut working_set = StateWorkingSet::new(engine_state);

        let output = parse(&mut working_set, None, commandline_args.as_bytes(), false);
        if let Some(err) = working_set.parse_errors.first() {
            report_error(&working_set, err);

            std::process::exit(1);
        }

        (output, working_set.render())
    };

    engine_state.merge_delta(delta)?;

    let mut stack = Stack::new();

    // We should have a successful parse now
    if let Some(pipeline) = block.pipelines.first() {
        if let Some(Expr::Call(call)) = pipeline.elements.first().map(|e| &e.expr.expr) {
            // let config_file = call.get_flag_expr("config");
            let list_tasks = call.has_flag(engine_state, &mut stack, "list")?;
            let quiet_execution = call.has_flag(engine_state, &mut stack, "quiet")?;
            let attach_stdin = call.has_flag(engine_state, &mut stack, "stdin")?;
            let show_help = call.has_flag(engine_state, &mut stack, "help")?;
            #[cfg(feature = "debug")]
            let debug_output = call.has_flag(engine_state, &mut stack, "debug")?;

            if call.has_flag(engine_state, &mut stack, "version")? {
                let version = env!("CARGO_PKG_VERSION").to_string();
                let _ = std::panic::catch_unwind(move || {
                    stdout_write_all_and_flush(format!("{version}\n"))
                });

                std::process::exit(0);
            }

            return Ok(NurArgs {
                list_tasks,
                quiet_execution,
                attach_stdin,
                show_help,
                #[cfg(feature = "debug")]
                debug_output,
            });
        }
    }

    // Just give the help and exit if the above fails
    let full_help = get_full_help(
        &Nur.signature(),
        &Nur.examples(),
        engine_state,
        &mut stack,
        true,
    );
    print!("{full_help}");
    std::process::exit(1);
}

#[derive(Debug, Clone)]
pub(crate) struct NurArgs {
    pub(crate) list_tasks: bool,
    pub(crate) quiet_execution: bool,
    pub(crate) attach_stdin: bool,
    pub(crate) show_help: bool,
    #[cfg(feature = "debug")]
    pub(crate) debug_output: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::init_engine_state;
    use tempfile::tempdir;

    #[test]
    fn test_gather_commandline_args_splits_on_task_name() {
        let args = vec![
            String::from("nur"),
            String::from("--quiet"),
            String::from("some_task_name"),
            String::from("--task-option"),
            String::from("task-value"),
        ];
        let (nur_args, has_task_call, task_call) = gather_commandline_args(args).unwrap();
        assert_eq!(nur_args, vec![String::from("nur"), String::from("--quiet")]);
        assert_eq!(has_task_call, true);
        assert_eq!(
            task_call,
            vec![
                String::from("nur"),
                String::from("some_task_name"),
                String::from("--task-option"),
                String::from("task-value")
            ]
        );
    }

    #[test]
    fn test_gather_commandline_args_handles_missing_nur_args() {
        let args = vec![
            String::from("nur"),
            String::from("some_task_name"),
            String::from("--task-option"),
            String::from("task-value"),
        ];
        let (nur_args, has_task_call, task_call) = gather_commandline_args(args).unwrap();
        assert_eq!(nur_args, vec![String::from("nur")]);
        assert_eq!(has_task_call, true);
        assert_eq!(
            task_call,
            vec![
                String::from("nur"),
                String::from("some_task_name"),
                String::from("--task-option"),
                String::from("task-value")
            ]
        );
    }

    #[test]
    fn test_gather_commandline_args_handles_missing_task_name() {
        let args = vec![String::from("nur"), String::from("--help")];
        let (nur_args, has_task_call, task_call) = gather_commandline_args(args).unwrap();
        assert_eq!(nur_args, vec![String::from("nur"), String::from("--help")]);
        assert_eq!(has_task_call, false);
        assert_eq!(task_call, vec![] as Vec<String>);
    }

    #[test]
    fn test_gather_commandline_args_handles_missing_task_args() {
        let args = vec![
            String::from("nur"),
            String::from("--quiet"),
            String::from("some_task_name"),
        ];
        let (nur_args, has_task_call, task_call) = gather_commandline_args(args).unwrap();
        assert_eq!(nur_args, vec![String::from("nur"), String::from("--quiet")]);
        assert_eq!(has_task_call, true);
        assert_eq!(
            task_call,
            vec![String::from("nur"), String::from("some_task_name")]
        );
    }

    #[test]
    fn test_gather_commandline_args_handles_no_args_at_all() {
        let args = vec![String::from("nur")];
        let (nur_args, has_task_call, task_call) = gather_commandline_args(args).unwrap();
        assert_eq!(nur_args, vec![String::from("nur")]);
        assert_eq!(has_task_call, false);
        assert_eq!(task_call, vec![] as Vec<String>);
    }

    fn _create_minimal_engine_for_erg_parsing() -> EngineState {
        let temp_dir = tempdir().unwrap();
        let temp_dir_path = temp_dir.path().to_path_buf();
        let engine_state = init_engine_state(&temp_dir_path).unwrap();

        engine_state
    }

    #[test]
    fn test_parse_commandline_args_without_args() {
        let mut engine_state = _create_minimal_engine_for_erg_parsing();

        let nur_args = parse_commandline_args("nur", &mut engine_state).unwrap();
        assert_eq!(nur_args.list_tasks, false);
        assert_eq!(nur_args.quiet_execution, false);
        assert_eq!(nur_args.attach_stdin, false);
        assert_eq!(nur_args.show_help, false);
    }

    #[test]
    fn test_parse_commandline_args_list() {
        let mut engine_state = _create_minimal_engine_for_erg_parsing();

        let nur_args = parse_commandline_args("nur --list", &mut engine_state).unwrap();
        assert_eq!(nur_args.list_tasks, true);
        assert_eq!(nur_args.quiet_execution, false);
        assert_eq!(nur_args.attach_stdin, false);
        assert_eq!(nur_args.show_help, false);
    }

    #[test]
    fn test_parse_commandline_args_quiet() {
        let mut engine_state = _create_minimal_engine_for_erg_parsing();

        let nur_args = parse_commandline_args("nur --quiet", &mut engine_state).unwrap();
        assert_eq!(nur_args.list_tasks, false);
        assert_eq!(nur_args.quiet_execution, true);
        assert_eq!(nur_args.attach_stdin, false);
        assert_eq!(nur_args.show_help, false);
    }

    #[test]
    fn test_parse_commandline_args_stdin() {
        let mut engine_state = _create_minimal_engine_for_erg_parsing();

        let nur_args = parse_commandline_args("nur --stdin", &mut engine_state).unwrap();
        assert_eq!(nur_args.list_tasks, false);
        assert_eq!(nur_args.quiet_execution, false);
        assert_eq!(nur_args.attach_stdin, true);
        assert_eq!(nur_args.show_help, false);
    }

    #[test]
    fn test_parse_commandline_args_help() {
        let mut engine_state = _create_minimal_engine_for_erg_parsing();

        let nur_args = parse_commandline_args("nur --help", &mut engine_state).unwrap();
        assert_eq!(nur_args.list_tasks, false);
        assert_eq!(nur_args.quiet_execution, false);
        assert_eq!(nur_args.attach_stdin, false);
        assert_eq!(nur_args.show_help, true);
    }
}
