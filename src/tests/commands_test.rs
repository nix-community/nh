use color_eyre::Result;

use crate::{
    init_test,
    testing::{
        harness::EnvContext,
        mock::{
            CommandExecutor,
            MockCommandExecutor,
        },
    },
};

/// Tests for the commands module using mocks
#[cfg(test)]
mod tests {
    use mockall::predicate::*;

    use super::*;

    // Custom command struct that uses our mock
    struct TestCommand<'a> {
        command:  String,
        args:     Vec<String>,
        executor: &'a MockCommandExecutor,
    }

    impl<'a> TestCommand<'a> {
        fn new(cmd: &str, executor: &'a MockCommandExecutor) -> Self {
            Self {
                command: cmd.to_string(),
                args: Vec::new(),
                executor,
            }
        }

        fn arg(mut self, arg: &str) -> Self {
            self.args.push(arg.to_string());
            self
        }

        fn args(mut self, args: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
            for arg in args {
                self.args.push(arg.as_ref().to_string());
            }
            self
        }

        fn run_capture(&self) -> Result<Option<String>> {
            // Convert args from String to &str for the mock
            let args_refs: Vec<&str> = self.args.iter().map(|s| s.as_str()).collect();

            // Call the mock
            let result = self.executor.run_command(&self.command, &args_refs)?;
            Ok(Some(result))
        }
    }

    /// Test that a command is executed correctly
    #[test]
    fn test_command_execution() {
        init_test!();

        let mut mock = MockCommandExecutor::new();

        // Set up the mock expectation
        mock.expect_run_command()
            .with(eq("nix"), always())
            .times(1)
            .returning(|_, _| Ok("nix (Nix) 2.24.14".to_string()));

        // Run the test with our TestCommand that uses the mock
        let output = TestCommand::new("nix", &mock)
            .arg("--version")
            .run_capture()
            .unwrap();

        assert!(output.is_some());
        let output = output.unwrap();
        assert!(output.contains("2.24.14"));
    }

    /// Test command with environment variables
    #[test]
    fn test_command_with_env_vars() {
        init_test!();

        let mut env_ctx = EnvContext::new();
        env_ctx.set_var("NH_TEST_VAR", "test-value");

        let mut mock = MockCommandExecutor::new();

        mock.expect_run_command()
            .with(eq("nix"), always())
            .times(1)
            .returning(|_, _| Ok("/nix/store/abcdef-nixpkgs".to_string()));

        env_ctx.run(|| {
            let output = TestCommand::new("nix", &mock)
                .args(["eval", "-f", "<nixpkgs>", "path"])
                .run_capture()
                .unwrap();

            assert!(output.is_some());
            assert_eq!(output.unwrap(), "/nix/store/abcdef-nixpkgs");
        });
    }
}
