#[macro_use]
extern crate include_dir;

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use log::debug;
use std::env;
use std::path::{Path, PathBuf};
use std::str::FromStr;

mod build;
mod cache;
mod cfg;
mod common;
mod opt;
mod templates;
mod exec;

mod editor;

pub fn opt_from_args(args: &[String]) -> opt::Opt {
    let mut args_iter = args.iter();
    args_iter.next();

    if let Some(script_src) = args_iter.next() {
        let absolute_script_src = common::script_src_to_absolute(Path::new(&script_src));
        if let Ok(absolute_script_src) = absolute_script_src {
            if absolute_script_src.exists() {
                let mut command: Vec<String> = vec![absolute_script_src.to_string_lossy().into()];
                command.append(&mut args_iter.cloned().collect());
                return opt::Opt { command, cmd: None };
            }
        }
    }

    let opts = opt::Opt::parse_from(args.iter());
    debug!("Parsed command line options: {:#?}", opts);

    if opts.cmd.is_none() && opts.command.is_empty() {
        opt::display_help();
    };
    opts
}

fn default_main(script_path: &str, args: &[String]) -> Result<()> {
    let build_mode_env = std::env::var_os("SCRIPTISTO_BUILD").unwrap_or_default();
    let build_mode = opt::BuildMode::from_str(&build_mode_env.to_string_lossy())?;
    let show_logs = std::env::var_os("SCRIPTISTO_BUILD_LOGS").is_some();

    let (cfg, script_cache_path) = build::perform(build_mode, script_path, show_logs)
        .context(format!("Build failed for {:?}", script_path))?;

    let mut full_target_bin = script_cache_path.clone();
    full_target_bin.push(PathBuf::from(cfg.target_bin));
    let full_target_bin = full_target_bin
        .canonicalize()?
        .to_string_lossy()
        .to_string();
    debug!("Full target_bin path: {:?}", full_target_bin);

    let (binary, mut target_argv) = match cfg.target_interpreter {
        Some(ref target_interpreter) if !target_interpreter.is_empty() => {
            let mut seq: Vec<String> = target_interpreter
                .split_ascii_whitespace()
                .map(|s| s.to_string())
                .collect();
            let binary = seq
                .first()
                .expect("first() should work as we checked the guard above")
                .clone();
            seq.drain(..1);
            seq.push(full_target_bin);
            (binary, seq)
        }
        _ => (full_target_bin, vec![]),
    };
    target_argv.insert(0, binary.clone());
    // args.drain(..2);
    target_argv.extend_from_slice(args);
    debug!("Running exec {:?}, Args: {:?}", binary, target_argv);

    // Scripts can use this to find other build artifacts
    env::set_var(build::SCRIPTISTO_CACHE_DIR_VAR, script_cache_path);

    let error = match exec::execvp(&binary, &target_argv) {
        exec::Error::Errno(e) => {
            anyhow!("Cannot execute target binary '{:?}': {:#?}", binary, e)
        }
        _ => anyhow!("Cannot exec"),
    };
    Err(error)
}

pub fn run_script(script_path: &str, args: &[String]) -> Result<()> {
    default_main(script_path, args)
}

pub fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let opts = opt_from_args(&args);
    debug!("Parsed options: {:?}", opts);

    match opts.cmd {
        None => {
            let mut command_iter = opts.command.iter();
            let mut script_src = command_iter.next().ok_or_else(|| {
                anyhow!("PROBABLY A BUG: script_src must be non-empty if no subcommand found.")
            })?.clone();
            // adjust script_src to relative or absolute path
            if !script_src.starts_with(['.', '/']) {
                script_src = format!("./{}", script_src);
            }
            let script_src = common::script_src_to_absolute(Path::new(&script_src))?;
            let args: Vec<String> = command_iter.cloned().collect();
            default_main(&script_src.to_string_lossy(), args.as_slice())
        }
        Some(opt::Command::Cache { cmd }) => cache::command_cache(cmd),
        Some(opt::Command::New { template_name }) => templates::command_new(template_name),
        Some(opt::Command::Template { cmd }) => templates::command_template(cmd),
        Some(opt::Command::Build {
                 script_src,
                 build_mode,
             }) => {
            let _ = build::perform(build_mode.unwrap_or_default(), &script_src, true);
            Ok(())
        }
    }
}
