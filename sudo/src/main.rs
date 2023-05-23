#![forbid(unsafe_code)]

use pam::authenticate;
use std::{env, process::exit, sync::atomic::Ordering};
use sudo_cli::{help, SudoAction, SudoOptions};
use sudo_common::{Context, Error};
use sudo_env::environment;
use sudo_exec::ExitReason;
use sudoers::{Authorization, DirChange, Judgement, Policy, PreJudgementPolicy, Sudoers};

mod diagnostic;
use diagnostic::diagnostic;
mod pam;

fn parse_sudoers() -> Result<Sudoers, Error> {
    // TODO: move to global configuration
    let sudoers_path = "/etc/sudoers.test";

    let (sudoers, syntax_errors) =
        Sudoers::new(sudoers_path).map_err(|e| Error::Configuration(format!("{e}")))?;

    for sudoers::Error(pos, error) in syntax_errors {
        diagnostic!("{error}", sudoers_path @ pos);
    }

    Ok(sudoers)
}

/// parse sudoers file and check permission to run the provided command given the context
fn check_sudoers(sudoers: &Sudoers, context: &Context) -> sudoers::Judgement {
    sudoers.check(
        &context.current_user,
        &context.hostname,
        sudoers::Request {
            user: &context.target_user,
            group: &context.target_group,
            command: &context.command.command,
            arguments: &context.command.arguments.join(" "),
        },
    )
}

/// Resolve the path to use and build a context object from the options
fn build_context(
    sudo_options: SudoOptions,
    sudoers: &impl PreJudgementPolicy,
) -> Result<Context, Error> {
    let env_path = env::var("PATH").unwrap_or_default();
    let path = sudoers.secure_path().unwrap_or(env_path);

    Context::build_from_options(sudo_options, path)
}

/// Change context values given a policy
fn apply_policy_to_context(context: &mut Context, policy: &Judgement) -> Result<(), Error> {
    // see if the chdir flag is permitted
    match policy.chdir() {
        DirChange::Any => {}
        DirChange::Strict(optdir) => {
            if context.chdir.is_some() {
                return Err(Error::auth("no permission")); // TODO better user error messages
            } else {
                context.chdir = optdir.map(std::path::PathBuf::from)
            }
        }
    }

    Ok(())
}

/// show warning message when SUDO_RS_IS_UNSTABLE is not set to the appropriate value
fn unstable_warning() {
    let check_var = std::env::var("SUDO_RS_IS_UNSTABLE").unwrap_or_else(|_| "".to_string());

    if check_var != "I accept that my system may break unexpectedly" {
        eprintln!(
            "WARNING!
Sudo-rs is in the early stages of development and could potentially break your system.
We recommend that you do not run this on any production environment. To turn off this
warning and start using sudo-rs set the environment variable SUDO_RS_IS_UNSTABLE to
the value `I accept that my system may break unexpectedly`. If you are unsure how to
do this then this software is not suited for you at this time."
        );

        std::process::exit(1);
    }
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn sudo_process() -> Result<(), Error> {
    sudo_log::SudoLogger::new().into_global_logger();

    // parse cli options
    let sudo_options = match SudoOptions::from_env() {
        Ok(options) => match options.action {
            SudoAction::Help => {
                eprintln!("{}", help::HELP_MSG);
                std::process::exit(0);
            }
            SudoAction::Version => {
                eprintln!("sudo-rs {VERSION}");
                std::process::exit(0);
            }
            SudoAction::Validate => {
                unimplemented!();
            }
            SudoAction::RemoveTimestamp => {
                unimplemented!();
            }
            SudoAction::ResetTimestamp => {
                unimplemented!();
            }
            SudoAction::Run(_) => options,
            SudoAction::List(_) => {
                unimplemented!();
            }
            SudoAction::Edit(_) => {
                unimplemented!();
            }
        },
        Err(e) => {
            eprintln!("{e}\n{}", help::USAGE_MSG);
            std::process::exit(1);
        }
    };

    unstable_warning();

    // parse sudoers file
    let sudoers = parse_sudoers()?;

    // build context given a path
    let mut context = build_context(sudo_options, &sudoers)?;

    // check sudoers file for permission
    let policy = check_sudoers(&sudoers, &context);

    // see if user must be authenticated
    match policy.authorization() {
        Authorization::Required => {
            // authenticate user using pam
            authenticate(&context)?;
        }
        Authorization::Passed => {}
        Authorization::Forbidden => {
            return Err(Error::auth(&format!(
                "I'm sorry {}. I'm afraid I can't do that",
                context.current_user.name
            )));
        }
    };

    apply_policy_to_context(&mut context, &policy)?;

    // build environment
    let current_env = std::env::vars_os().collect();
    let target_env = environment::get_target_environment(current_env, &context, &policy);

    let pid = context.process.pid;

    // run command and return corresponding exit code
    let (reason, emulate_default_handler) = sudo_exec::run_command(context, target_env)?;

    // Run any clean-up code before this line.
    emulate_default_handler.store(true, Ordering::SeqCst);

    match reason {
        ExitReason::Code(code) => exit(code),
        ExitReason::Signal(signal) => {
            sudo_system::kill(pid, signal)?;
        }
    }

    Ok(())
}

fn main() {
    match sudo_process() {
        Ok(()) => (),
        Err(error) => {
            diagnostic!("{error}");
            std::process::exit(1);
        }
    }
}
