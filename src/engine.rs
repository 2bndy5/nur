use crate::args::{parse_commandline_args, NurArgs};
use crate::errors::{NurError, NurResult};
use crate::names::{
    NUR_ENV_NU_LIB_DIRS, NUR_NAME, NUR_VAR_CONFIG_DIR, NUR_VAR_DEFAULT_LIB_DIR,
    NUR_VAR_PROJECT_PATH, NUR_VAR_RUN_PATH, NUR_VAR_TASK_NAME,
};
use crate::nu_version::NU_VERSION;
use crate::scripts::{get_default_nur_config, get_default_nur_env};
use crate::state::NurState;
use nu_cli::gather_parent_env_vars;
use nu_engine::get_full_help;
use nu_protocol::ast::Block;
use nu_protocol::engine::{Command, Stack, StateWorkingSet};
use nu_protocol::eval_const::create_nu_constant;
use nu_protocol::{
    engine::EngineState, report_error, report_error_new, PipelineData, Record, Span, Type, Value,
    NU_VARIABLE_ID,
};
use nu_std::load_standard_library;
use nu_utils::stdout_write_all_and_flush;
use std::fs;
use std::path::Path;
use std::sync::Arc;

pub(crate) fn init_engine_state<P: AsRef<Path>>(project_path: P) -> NurResult<EngineState> {
    let engine_state = nu_cmd_lang::create_default_context();
    let engine_state = nu_command::add_shell_command_context(engine_state);
    let engine_state = nu_cmd_extra::add_extra_command_context(engine_state);
    #[cfg(feature = "dataframe")]
    let engine_state = nu_cmd_dataframe::add_dataframe_context(engine_state);
    let engine_state = nu_cli::add_cli_context(engine_state);
    let engine_state = nu_explore::add_explore_context(engine_state);
    let engine_state = crate::commands::create_nu_context(engine_state);
    let engine_state = crate::commands::create_nur_context(engine_state);

    // Prepare engine state to be changed
    let mut engine_state = engine_state;

    // First, set up env vars as strings only
    gather_parent_env_vars(&mut engine_state, project_path.as_ref());
    engine_state.add_env_var(
        "NU_VERSION".to_string(),
        Value::string(NU_VERSION, Span::unknown()),
    );

    // Load std library
    if load_standard_library(&mut engine_state).is_err() {
        return Err(NurError::InitError(String::from(
            "Could not load std library",
        )));
    }

    // Set some engine flags
    engine_state.is_interactive = false;
    engine_state.is_login = false;
    engine_state.history_enabled = false;

    Ok(engine_state)
}

#[derive(Clone)]
pub(crate) struct NurEngine {
    pub(crate) engine_state: EngineState,
    pub(crate) stack: Stack,

    pub(crate) state: NurState,
}

impl NurEngine {
    pub(crate) fn new(engine_state: EngineState, nur_state: NurState) -> NurResult<NurEngine> {
        let mut nur_engine = NurEngine {
            engine_state,
            stack: Stack::new(),

            state: nur_state,
        };

        nur_engine._apply_nur_state()?;

        Ok(nur_engine)
    }

    fn _apply_nur_state(&mut self) -> NurResult<()> {
        // Set default scripts path
        self.engine_state.add_env_var(
            NUR_ENV_NU_LIB_DIRS.to_string(),
            Value::test_string(self.state.lib_dir_path.to_string_lossy()),
        );

        // Set config and env paths to .nur versions
        self.engine_state
            .set_config_path("env-path", self.state.env_path.clone());
        self.engine_state
            .set_config_path("config-path", self.state.config_path.clone());

        // Set up the $nu constant before evaluating any files (need to have $nu available in them)
        let nu_const = create_nu_constant(
            &self.engine_state,
            PipelineData::empty().span().unwrap_or_else(Span::unknown),
        )?;
        self.engine_state
            .set_variable_const_val(NU_VARIABLE_ID, nu_const);

        // Set up the $nur constant record (like $nu)
        let mut nur_record = Record::new();
        nur_record.push(
            NUR_VAR_RUN_PATH,
            Value::string(
                String::from(self.state.run_path.to_str().unwrap()),
                Span::unknown(),
            ),
        );
        nur_record.push(
            NUR_VAR_PROJECT_PATH,
            Value::string(
                String::from(self.state.project_path.to_str().unwrap()),
                Span::unknown(),
            ),
        );
        nur_record.push(
            NUR_VAR_TASK_NAME,
            Value::string(&self.state.task_name, Span::unknown()),
        );
        nur_record.push(
            NUR_VAR_CONFIG_DIR,
            Value::string(
                String::from(self.state.config_dir.to_str().unwrap()),
                Span::unknown(),
            ),
        );
        nur_record.push(
            NUR_VAR_DEFAULT_LIB_DIR,
            Value::string(
                String::from(self.state.lib_dir_path.to_str().unwrap()),
                Span::unknown(),
            ),
        );
        let mut working_set = StateWorkingSet::new(&self.engine_state);
        let nur_var_id = working_set.add_variable(
            NUR_NAME.as_bytes().into(),
            Span::unknown(),
            Type::Any,
            false,
        );
        self.stack
            .add_var(nur_var_id, Value::record(nur_record, Span::unknown()));
        self.engine_state.merge_delta(working_set.render())?;

        Ok(())
    }

    pub(crate) fn parse_args(&mut self) -> NurArgs {
        parse_commandline_args(&self.state.args_to_nur.join(" "), &mut self.engine_state)
            .unwrap_or_else(|_| std::process::exit(1))
    }

    pub(crate) fn load_env(&mut self) -> NurResult<()> {
        if self.state.env_path.exists() {
            self.source_and_merge_env(self.state.env_path.clone(), PipelineData::empty())?;
        } else {
            self.eval_and_merge_env(get_default_nur_env(), PipelineData::empty())?;
        }

        Ok(())
    }

    pub(crate) fn load_config(&mut self) -> NurResult<()> {
        if self.state.config_path.exists() {
            self.source_and_merge_env(self.state.config_path.clone(), PipelineData::empty())?;
        } else {
            self.eval_and_merge_env(get_default_nur_config(), PipelineData::empty())?;
        }

        Ok(())
    }

    pub(crate) fn load_nurfiles(&mut self) -> NurResult<()> {
        if self.state.nurfile_path.exists() {
            self.source(self.state.nurfile_path.clone(), PipelineData::empty())?;
        }
        if self.state.local_nurfile_path.exists() {
            self.source(self.state.local_nurfile_path.clone(), PipelineData::empty())?;
        }

        Ok(())
    }

    fn _parse_nu_script(
        &mut self,
        file_path: Option<&str>,
        contents: String,
    ) -> NurResult<Arc<Block>> {
        if file_path.is_some() {
            self.engine_state.start_in_file(file_path);
        }

        let mut working_set = StateWorkingSet::new(&self.engine_state);
        let block = nu_parser::parse(&mut working_set, file_path, &contents.into_bytes(), false);

        if working_set.parse_errors.is_empty() {
            let delta = working_set.render();
            self.engine_state.merge_delta(delta)?;

            Ok(block)
        } else {
            if let Some(err) = working_set.parse_errors.first() {
                report_error(&working_set, err);
                std::process::exit(1);
            }

            Err(NurError::ParseErrors(working_set.parse_errors))
        }
    }

    fn _execute_block(&mut self, block: &Block, input: PipelineData) -> NurResult<PipelineData> {
        nu_engine::get_eval_block(&self.engine_state)(
            &self.engine_state,
            &mut self.stack,
            block,
            input,
        )
        .map_err(|err| {
            report_error_new(&self.engine_state, &err);
            std::process::exit(1);
        })
    }

    fn _eval<S: ToString>(
        &mut self,
        file_path: Option<&str>,
        contents: S,
        input: PipelineData,
        print: bool,
        merge_env: bool,
    ) -> NurResult<i64> {
        let str_contents = contents.to_string();

        if str_contents.is_empty() {
            return Ok(0);
        }

        let block = self._parse_nu_script(file_path, str_contents)?;

        let result = self._execute_block(&block, input)?;

        // Merge env is requested
        if merge_env {
            match nu_engine::env::current_dir(&self.engine_state, &self.stack) {
                Ok(cwd) => {
                    if let Err(e) = self.engine_state.merge_env(&mut self.stack, cwd) {
                        let working_set = StateWorkingSet::new(&self.engine_state);
                        report_error(&working_set, &e);
                    }
                }
                Err(e) => {
                    let working_set = StateWorkingSet::new(&self.engine_state);
                    report_error(&working_set, &e);
                }
            }
        }

        // Print result is requested
        if print {
            let exit_code = result.print(&self.engine_state, &mut self.stack, false, false)?;
            Ok(exit_code)
        } else {
            if let PipelineData::ExternalStream {
                exit_code: Some(exit_code),
                ..
            } = result
            {
                let mut exit_codes: Vec<_> = exit_code.into_iter().collect();
                return match exit_codes.pop() {
                    #[cfg(unix)]
                    Some(Value::Error { error, .. }) => Err(NurError::from(*error)),
                    Some(Value::Int { val, .. }) => Ok(val),
                    _ => Ok(0),
                };
            }
            Ok(0)
        }
    }

    // This is used in tests only currently
    #[allow(dead_code)]
    pub fn eval<S: ToString>(&mut self, contents: S, input: PipelineData) -> NurResult<i64> {
        self._eval(None, contents, input, false, false)
    }

    pub(crate) fn eval_and_print<S: ToString>(
        &mut self,
        contents: S,
        input: PipelineData,
    ) -> NurResult<i64> {
        self._eval(None, contents, input, true, false)
    }

    pub(crate) fn eval_and_merge_env<S: ToString>(
        &mut self,
        contents: S,
        input: PipelineData,
    ) -> NurResult<i64> {
        self._eval(None, contents, input, false, true)
    }

    pub(crate) fn source<P: AsRef<Path>>(
        &mut self,
        file_path: P,
        input: PipelineData,
    ) -> NurResult<i64> {
        let contents = fs::read_to_string(&file_path)?;

        self._eval(file_path.as_ref().to_str(), contents, input, false, false)
    }

    pub(crate) fn source_and_merge_env<P: AsRef<Path>>(
        &mut self,
        file_path: P,
        input: PipelineData,
    ) -> NurResult<i64> {
        let contents = fs::read_to_string(&file_path)?;

        self._eval(file_path.as_ref().to_str(), contents, input, false, true)
    }

    pub(crate) fn has_def<S: AsRef<str>>(&self, name: S) -> bool {
        self.engine_state
            .find_decl(name.as_ref().as_bytes(), &[])
            .is_some()
    }

    pub(crate) fn get_def<S: AsRef<str>>(&self, name: S) -> Option<&dyn Command> {
        if let Some(decl_id) = self.engine_state.find_decl(name.as_ref().as_bytes(), &[]) {
            Some(self.engine_state.get_decl(decl_id))
        } else {
            None
        }
    }

    pub(crate) fn print_help(&mut self, command: &dyn Command) {
        let full_help = get_full_help(
            &command.signature(),
            &command.examples(),
            &self.engine_state,
            &mut self.stack,
            true,
        );

        let _ = std::panic::catch_unwind(move || stdout_write_all_and_flush(full_help));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::{
        NUR_CONFIG_CONFIG_FILENAME, NUR_CONFIG_DIR, NUR_CONFIG_ENV_FILENAME, NUR_CONFIG_LIB_PATH,
        NUR_FILE, NUR_LOCAL_FILE,
    };
    use std::fs::File;
    use std::io::Write;
    use tempfile::{tempdir, TempDir};

    fn _has_decl<S: AsRef<str>>(engine_state: &mut EngineState, name: S) -> bool {
        engine_state
            .find_decl(name.as_ref().as_bytes(), &[])
            .is_some()
    }

    #[test]
    fn test_init_engine_state_will_add_commands() {
        let temp_dir = tempdir().unwrap();
        let temp_dir_path = temp_dir.path().to_path_buf();
        let mut engine_state = init_engine_state(&temp_dir_path).unwrap();

        assert!(_has_decl(&mut engine_state, "alias"));
        assert!(_has_decl(&mut engine_state, "do"));
        assert!(_has_decl(&mut engine_state, "uniq"));
        assert!(_has_decl(&mut engine_state, "help"));
        assert!(_has_decl(&mut engine_state, "str"));
        assert!(_has_decl(&mut engine_state, "format pattern"));
        assert!(_has_decl(&mut engine_state, "history"));
        assert!(_has_decl(&mut engine_state, "explore"));
        assert!(_has_decl(&mut engine_state, "print"));
        assert!(_has_decl(&mut engine_state, "nu-highlight"));
        assert!(_has_decl(&mut engine_state, "nur"));
    }

    #[test]
    fn test_init_engine_state_will_set_nu_version() {
        let temp_dir = tempdir().unwrap();
        let temp_dir_path = temp_dir.path().to_path_buf();
        let engine_state = init_engine_state(&temp_dir_path).unwrap();

        assert!(engine_state.get_env_var("NU_VERSION").is_some());
    }

    #[test]
    fn test_init_engine_state_will_add_std_lib() {
        let temp_dir = tempdir().unwrap();
        let temp_dir_path = temp_dir.path().to_path_buf();
        let engine_state = init_engine_state(&temp_dir_path).unwrap();

        assert!(engine_state
            .find_module("std".as_bytes(), &[vec![]],)
            .is_some());
    }

    #[test]
    fn test_init_engine_state_will_set_flags() {
        let temp_dir = tempdir().unwrap();
        let temp_dir_path = temp_dir.path().to_path_buf();
        let engine_state = init_engine_state(&temp_dir_path).unwrap();

        assert_eq!(engine_state.is_interactive, false);
        assert_eq!(engine_state.is_login, false);
        assert_eq!(engine_state.history_enabled, false);
    }

    fn _prepare_nur_engine(temp_dir: &TempDir) -> NurEngine {
        let temp_dir_path = temp_dir.path().to_path_buf();
        let nurfile_path = temp_dir.path().join(NUR_FILE);
        File::create(&nurfile_path).unwrap();

        let args = vec![String::from("nur"), String::from("some_task")];
        let nur_state = NurState::new(temp_dir_path.clone(), args);
        let engine_state = init_engine_state(temp_dir_path).unwrap();

        NurEngine::new(engine_state, nur_state).unwrap()
    }

    fn _cleanup_nur_engine(temp_dir: &TempDir) {
        let nurfile_path = temp_dir.path().join(NUR_FILE);
        let nurfile_local_path = temp_dir.path().join(NUR_FILE);
        let config_dir = temp_dir.path().join(NUR_CONFIG_DIR);

        fs::remove_file(nurfile_path).unwrap();
        if nurfile_local_path.exists() {
            fs::remove_file(nurfile_local_path).unwrap();
        }
        if config_dir.exists() {
            fs::remove_dir_all(config_dir).unwrap();
        }
    }

    fn _has_var<S: AsRef<str>>(nur_engine: &mut NurEngine, name: S) -> bool {
        let name = name.as_ref();
        let dollar_name = format!("${name}");
        let var_id = nur_engine
            .engine_state
            .active_overlays(&vec![])
            .find_map(|o| {
                o.vars
                    .get(dollar_name.as_bytes())
                    .or(o.vars.get(name.as_bytes()))
            })
            .unwrap();

        nur_engine.stack.get_var(*var_id, Span::unknown()).is_ok()
    }

    #[test]
    fn test_nur_engine_will_set_nur_variable() {
        let temp_dir = tempdir().unwrap();
        let mut nur_engine = _prepare_nur_engine(&temp_dir);

        assert!(_has_var(&mut nur_engine, "nur"));

        _cleanup_nur_engine(&temp_dir);
    }

    #[test]
    fn test_nur_engine_will_load_nurfiles() {
        let temp_dir = tempdir().unwrap();
        let mut nur_engine = _prepare_nur_engine(&temp_dir);

        let nurfile_path = temp_dir.path().join(NUR_FILE);
        let mut nurfile = File::create(&nurfile_path).unwrap();
        nurfile.write_all(b"def nurfile-command [] {}").unwrap();
        let nurfile_local_path = temp_dir.path().join(NUR_LOCAL_FILE);
        let mut nurfile_local = File::create(&nurfile_local_path).unwrap();
        nurfile_local
            .write_all(b"def nurfile-local-command [] {}")
            .unwrap();

        nur_engine.load_env().unwrap();
        nur_engine.load_config().unwrap();
        nur_engine.load_nurfiles().unwrap();

        assert!(_has_decl(&mut nur_engine.engine_state, "nurfile-command"));
        assert!(_has_decl(
            &mut nur_engine.engine_state,
            "nurfile-local-command"
        ));

        _cleanup_nur_engine(&temp_dir);
    }

    #[test]
    fn test_nur_engine_will_load_env_and_config() {
        let temp_dir = tempdir().unwrap();
        let mut nur_engine = _prepare_nur_engine(&temp_dir);

        let config_dir = temp_dir.path().join(NUR_CONFIG_DIR);
        fs::create_dir(config_dir.clone()).unwrap();
        let env_path = config_dir.join(NUR_CONFIG_ENV_FILENAME);
        let mut env_file = File::create(&env_path).unwrap();
        env_file.write_all(b"def env-command [] {}").unwrap();
        let config_path = config_dir.join(NUR_CONFIG_CONFIG_FILENAME);
        let mut config_file = File::create(&config_path).unwrap();
        config_file.write_all(b"def config-command [] {}").unwrap();

        nur_engine.load_env().unwrap();
        nur_engine.load_config().unwrap();
        nur_engine.load_nurfiles().unwrap();

        assert!(_has_decl(&mut nur_engine.engine_state, "env-command"));
        assert!(_has_decl(&mut nur_engine.engine_state, "config-command"));

        _cleanup_nur_engine(&temp_dir);
    }

    #[test]
    fn test_nur_engine_will_allow_scripts() {
        let temp_dir = tempdir().unwrap();
        let mut nur_engine = _prepare_nur_engine(&temp_dir);

        let config_dir = temp_dir.path().join(NUR_CONFIG_DIR);
        fs::create_dir(config_dir.clone()).unwrap();
        let scripts_dir = config_dir.join(NUR_CONFIG_LIB_PATH);
        fs::create_dir(scripts_dir.clone()).unwrap();
        let module_path = scripts_dir.join("test-module.nu");
        let mut module_file = File::create(&module_path).unwrap();
        module_file
            .write_all(b"export def module-command [] {}")
            .unwrap();

        nur_engine.load_env().unwrap();
        nur_engine.load_config().unwrap();
        nur_engine.load_nurfiles().unwrap();

        nur_engine
            .eval("use test-module.nu *", PipelineData::empty())
            .unwrap();

        assert!(_has_decl(&mut nur_engine.engine_state, "module-command"));

        _cleanup_nur_engine(&temp_dir);
    }
}
