use crate::args::gather_commandline_args;
use crate::errors::NurResult;
use crate::names::{
    NUR_CONFIG_CONFIG_FILENAME, NUR_CONFIG_DIR, NUR_CONFIG_ENV_FILENAME, NUR_CONFIG_LIB_PATH,
    NUR_FILE, NUR_LOCAL_FILE,
};
use crate::path::find_project_path;
use std::path::PathBuf;

#[derive(Clone)]
pub(crate) struct NurState {
    pub(crate) run_path: PathBuf,
    pub(crate) has_project_path: bool,
    pub(crate) project_path: PathBuf,

    pub(crate) config_dir: PathBuf,
    pub(crate) lib_dir_path: PathBuf,
    pub(crate) env_path: PathBuf,
    pub(crate) config_path: PathBuf,

    pub(crate) nurfile_path: PathBuf,
    pub(crate) local_nurfile_path: PathBuf,

    pub(crate) args_to_nur: Vec<String>,
    pub(crate) has_task_call: bool,
    pub(crate) task_call: Vec<String>,
    pub(crate) task_name: Option<String>, // full task name, like "nur some-task"
}

impl NurState {
    pub(crate) fn new(run_path: PathBuf, args: Vec<String>) -> NurResult<Self> {
        // Get initial directory details
        let found_project_path = find_project_path(&run_path);
        let has_project_path = found_project_path.is_some();
        let project_path = found_project_path.unwrap_or(run_path.clone());

        // Set all paths
        let config_dir = project_path.join(NUR_CONFIG_DIR);
        let lib_dir_path = config_dir.join(NUR_CONFIG_LIB_PATH);
        let env_path = config_dir.join(NUR_CONFIG_ENV_FILENAME);
        let config_path = config_dir.join(NUR_CONFIG_CONFIG_FILENAME);

        // Set nurfiles
        let nurfile_path = project_path.join(NUR_FILE);
        let local_nurfile_path = project_path.join(NUR_LOCAL_FILE);

        // Parse args into bits
        let (args_to_nur, has_task_call, task_call) = gather_commandline_args(args)?;

        Ok(NurState {
            run_path,
            has_project_path,
            project_path,

            config_dir,
            lib_dir_path,
            env_path,
            config_path,

            nurfile_path,
            local_nurfile_path,

            args_to_nur,
            has_task_call,
            task_call,
            task_name: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_nur_state_with_project_path() {
        let temp_dir = tempdir().unwrap();
        let temp_dir_path = temp_dir.path().to_path_buf();
        let nurfile_path = temp_dir.path().join(NUR_FILE);
        File::create(&nurfile_path).unwrap();

        // Setup test
        let args = vec![
            String::from("nur"),
            String::from("--quiet"),
            String::from("some_task"),
            String::from("task_arg"),
        ];
        let state = NurState::new(temp_dir_path.clone(), args).unwrap();

        // Check everything works out
        assert_eq!(state.run_path, temp_dir_path);
        assert_eq!(state.project_path, temp_dir_path);
        assert_eq!(state.has_project_path, true);

        assert_eq!(state.config_dir, temp_dir_path.join(".nur"));
        assert_eq!(state.lib_dir_path, temp_dir_path.join(".nur/scripts"));
        assert_eq!(state.env_path, temp_dir_path.join(".nur/env.nu"));
        assert_eq!(state.config_path, temp_dir_path.join(".nur/config.nu"));

        assert_eq!(state.nurfile_path, temp_dir_path.join("nurfile"));
        assert_eq!(
            state.local_nurfile_path,
            temp_dir_path.join("nurfile.local")
        );

        assert_eq!(
            state.args_to_nur,
            vec![String::from("nur"), String::from("--quiet"),]
        );
        assert_eq!(state.has_task_call, true);
        assert_eq!(
            state.task_call,
            vec![
                String::from("nur"),
                String::from("some_task"),
                String::from("task_arg")
            ]
        );

        // Clean up
        std::fs::remove_file(nurfile_path).unwrap();
    }

    #[test]
    fn test_nur_state_without_project_path() {
        let temp_dir = tempdir().unwrap();
        let temp_dir_path = temp_dir.path().to_path_buf();

        // Setup test
        let args = vec![
            String::from("nur"),
            String::from("--quiet"),
            String::from("some_task"),
            String::from("task_arg"),
        ];
        let state = NurState::new(temp_dir_path.clone(), args).unwrap();

        // Check everything works out
        assert_eq!(state.run_path, temp_dir_path);
        assert_eq!(state.project_path, temp_dir_path); // same as run_path, as this is the fallback
        assert_eq!(state.has_project_path, false);

        assert_eq!(state.config_dir, temp_dir_path.join(".nur"));
        assert_eq!(state.lib_dir_path, temp_dir_path.join(".nur/scripts"));
        assert_eq!(state.env_path, temp_dir_path.join(".nur/env.nu"));
        assert_eq!(state.config_path, temp_dir_path.join(".nur/config.nu"));

        assert_eq!(state.nurfile_path, temp_dir_path.join("nurfile"));
        assert_eq!(
            state.local_nurfile_path,
            temp_dir_path.join("nurfile.local")
        );

        assert_eq!(
            state.args_to_nur,
            vec![String::from("nur"), String::from("--quiet"),]
        );
        assert_eq!(state.has_task_call, true);
        assert_eq!(
            state.task_call,
            vec![
                String::from("nur"),
                String::from("some_task"),
                String::from("task_arg")
            ]
        );
    }

    #[test]
    fn test_nur_state_without_task() {
        let temp_dir = tempdir().unwrap();
        let temp_dir_path = temp_dir.path().to_path_buf();

        // Setup test
        let args = vec![String::from("nur"), String::from("--help")];
        let state = NurState::new(temp_dir_path.clone(), args).unwrap();

        // Check everything works out
        assert_eq!(state.run_path, temp_dir_path);
        assert_eq!(state.project_path, temp_dir_path); // same as run_path, as this is the fallback
        assert_eq!(state.has_project_path, false);

        assert_eq!(state.config_dir, temp_dir_path.join(".nur"));
        assert_eq!(state.lib_dir_path, temp_dir_path.join(".nur/scripts"));
        assert_eq!(state.env_path, temp_dir_path.join(".nur/env.nu"));
        assert_eq!(state.config_path, temp_dir_path.join(".nur/config.nu"));

        assert_eq!(state.nurfile_path, temp_dir_path.join("nurfile"));
        assert_eq!(
            state.local_nurfile_path,
            temp_dir_path.join("nurfile.local")
        );

        assert_eq!(
            state.args_to_nur,
            vec![String::from("nur"), String::from("--help"),]
        );
        assert_eq!(state.has_task_call, false);
        assert_eq!(state.task_call, vec![] as Vec<String>);
    }
}
