// Copyright 2019 The Scriptisto Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use anyhow::{anyhow, Context, Result};
use log::debug;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::cfg;
use crate::common;
use crate::opt;

pub const SCRIPTISTO_CACHE_DIR_VAR: &str = "SCRIPTISTO_CACHE_DIR";
pub const SCRIPTISTO_SOURCE_DIR_VAR: &str = "SCRIPTISTO_SOURCE_DIR";
pub const SCRIPTISTO_SOURCE_VAR: &str = "SCRIPTISTO_SOURCE";

fn docker_prefix(script_cache_path: &Path) -> Result<String> {
    Ok(format!(
        "scriptisto-{}-{:x}",
        script_cache_path
            .file_name()
            .ok_or_else(|| anyhow!("BUG: invalid script_cache_path={:?}", script_cache_path))?
            .to_string_lossy(),
        md5::compute(script_cache_path.to_string_lossy().as_bytes())
    ))
}

pub fn docker_image_name(script_cache_path: &Path) -> Result<String> {
    docker_prefix(script_cache_path)
}

pub fn docker_volume_name(script_cache_path: &Path) -> Result<String> {
    let docker_prefix = docker_prefix(script_cache_path)?;
    Ok(format!("{}-src", docker_prefix))
}

fn docker_create_volume(
    volume_name: &str,
    script_cache_path: &Path,
    stderr_mode: Stdio,
) -> Result<()> {
    let mut build_vol_cmd = Command::new("docker");
    build_vol_cmd.arg("volume").arg("create").arg(volume_name);
    common::run_command(script_cache_path, build_vol_cmd, stderr_mode)?;
    Ok(())
}

fn docker_volume_cmd(
    volume_name: &str,
    script_cache_path: &Path,
    _run_as_current_user: bool,
    cmd: &str,
    stderr_mode: Stdio,
) -> Result<()> {
    let mut vol_cmd = Command::new("docker");
    vol_cmd.args(["run", "-t", "--rm"]);
    #[cfg(target_family = "unix")]
    if _run_as_current_user {
        unsafe {
            let uid = libc::getuid();
            vol_cmd.args(["-u", &format!("{}", uid)]);
        }
    }
    vol_cmd.args([
        "-v",
        &format!("{}:/vol", &volume_name),
        "-v",
        &format!("{}:/src", &script_cache_path.to_string_lossy()),
        "busybox",
        "sh",
        "-c",
        cmd,
    ]);
    common::run_command(script_cache_path, vol_cmd, stderr_mode)?;
    Ok(())
}

fn run_build_command<F>(
    cfg: &cfg::BuildSpec,
    script_path: &Path,
    script_cache_path: &Path,
    first_run: bool,
    build_mode: opt::BuildMode,
    stderr_mode: F,
) -> Result<()>
    where
        F: Fn() -> Stdio,
{
    if first_run || build_mode == opt::BuildMode::Full {
        if let Some(build_once_cmd) = &cfg.build_once_cmd {
            let mut cmd = Command::new("/bin/sh");
            cmd.arg("-c").arg(build_once_cmd);
            common::run_command(script_cache_path, cmd, stderr_mode())?;
        }
    }

    if let Some(build_cmd) = &cfg.build_cmd {
        match &cfg.docker_build {
            // TODO: Do better validation for empty dockerfile, but not-empty docker_build.
            Some(docker_build) if docker_build.dockerfile.is_some() => {
                // Write Dockerfile.
                let tmp_dockerfile_name = "Dockerfile.scriptisto";
                common::write_bytes(
                    script_cache_path,
                    &PathBuf::from(&tmp_dockerfile_name),
                    docker_build.dockerfile.clone().unwrap().as_bytes(),
                )?;

                // Create and populate sources volume.
                let src_docker_volume = docker_volume_name(script_cache_path)?;

                docker_create_volume(&src_docker_volume, script_cache_path, stderr_mode())?;

                docker_volume_cmd(
                    &src_docker_volume,
                    script_cache_path,
                    false,
                    "cp -rf /src/* /vol/",
                    stderr_mode(),
                )?;

                // Build temporary image.
                let tmp_docker_image = docker_image_name(script_cache_path)?;

                let mut build_im_cmd = Command::new("docker");
                build_im_cmd.arg("build");

                if build_mode == opt::BuildMode::Full {
                    build_im_cmd.arg("--no-cache");
                }

                build_im_cmd
                    .arg("-t")
                    .arg(&tmp_docker_image)
                    .arg("--label")
                    .arg(format!(
                        "scriptisto-cache-path={}",
                        script_cache_path.to_string_lossy()
                    ))
                    .arg("-f")
                    .arg(tmp_dockerfile_name)
                    .arg(".");

                common::run_command(script_cache_path, build_im_cmd, stderr_mode())?;

                // Build binary in Docker.
                let mut cmd = Command::new("docker");
                cmd.arg("run")
                    .arg("-t")
                    .arg("--rm")
                    .arg("--env")
                    .arg(format!(
                        "{}={}",
                        SCRIPTISTO_SOURCE_VAR,
                        &script_path.to_string_lossy()
                    ));

                if let Some(src_mount_dir) = &docker_build.src_mount_dir {
                    cmd.arg("-v")
                        .arg(format!("{}:{}", src_docker_volume, src_mount_dir));
                }

                cmd.args(docker_build.extra_args.iter())
                    .arg(tmp_docker_image)
                    .arg("sh")
                    .arg("-c")
                    .arg(build_cmd);

                common::run_command(script_cache_path, cmd, stderr_mode())?;

                // Extract target_bin back to host.
                let mut vol_path = PathBuf::from("/vol");
                vol_path.push(&cfg.target_bin);
                let mut src_path = PathBuf::from("/src");
                src_path.push(&cfg.target_bin);
                docker_volume_cmd(
                    &src_docker_volume,
                    script_cache_path,
                    true,
                    &format!(
                        "mkdir -p $(dirname {}) && cp -rf {} {}",
                        src_path.to_string_lossy(),
                        vol_path.to_string_lossy(),
                        src_path.to_string_lossy(),
                    ),
                    stderr_mode(),
                )?;
            }
            // Non-Docker build.
            _ => {
                let script_dir = match script_path.parent() {
                    None => {
                        return Err(anyhow!(
                            "Failed to look up parent directory of {:?}",
                            script_path
                        ));
                    }
                    Some(p) => p,
                };

                let mut cmd = Command::new("/bin/sh");
                cmd.arg("-c")
                    .arg(build_cmd)
                    .env(SCRIPTISTO_CACHE_DIR_VAR, script_cache_path)
                    .env(SCRIPTISTO_SOURCE_DIR_VAR, script_dir)
                    .env(SCRIPTISTO_SOURCE_VAR, script_path);

                let working_directory = if cfg.build_in_script_dir {
                    script_dir
                } else {
                    script_cache_path
                };

                common::run_command(working_directory, cmd, stderr_mode())?;
            }
        }
    }

    common::write_bytes(
        script_cache_path,
        &PathBuf::from("scriptisto.metadata"),
        String::new().as_bytes(),
    )
        .context("Cannot write metadata file")?;

    Ok(())
}

pub fn perform(
    build_mode: opt::BuildMode,
    script_path: &str,
    show_logs: bool,
) -> Result<(cfg::BuildSpec, PathBuf)> {
    let script_path = Path::new(script_path);

    let script_body = std::fs::read(script_path).context("Cannot read script file")?;
    let script_cache_path = common::build_cache_path(script_path).context(format!(
        "Cannot build cache path for script: {:?}",
        script_path
    ))?;
    debug!("Path: {:?}", script_path);
    debug!("Cache path: {:?}", script_cache_path);
    let mut cfg = cfg::BuildSpec::new(&script_body).context("Cannot parse build spec")?;

    let mut metadata_path = script_cache_path.clone();
    metadata_path.push("scriptisto.metadata");
    let metadata_modified = common::file_modified(&metadata_path).ok();
    let script_modified = common::file_modified(script_path).ok();

    // If source file is older than metadata file, also hash other paths. Those could be additional
    // inputs for the build, for example, which must be considered for triggering a rebuild.
    let mut additional_paths_max_modified: Option<std::time::SystemTime> = None;
    if metadata_modified > script_modified {
        let full_script_path = script_path
            .canonicalize()
            .context("Cannot build full path from given script path")?;
        let script_dir = full_script_path
            .parent()
            .expect("script_src has no parent directory");

        let mut num_additional_paths_scanned = 0;
        for additional_path in cfg.extra_src_paths.iter() {
            let mut full_additional_path = PathBuf::from(additional_path);
            if !full_additional_path.is_absolute() {
                full_additional_path = script_dir.join(additional_path);
            }

            debug!("Hashing additional path {:?}", full_additional_path);

            for entry_res in walkdir::WalkDir::new(&full_additional_path)
                .follow_links(true)
                .into_iter()
            {
                debug!("Checking directory entry {:?}", entry_res);
                if entry_res.is_err() {
                    continue;
                }
                let entry = entry_res.as_ref().unwrap();

                let metadata_res = entry.metadata();
                if metadata_res.is_ok() && metadata_res.as_ref().unwrap().is_file() {
                    num_additional_paths_scanned += 1;
                    if num_additional_paths_scanned > 500000 {
                        panic!("Too many files scanned");
                    }
                    let modified_res = metadata_res.as_ref().unwrap().modified();
                    if modified_res.is_ok()
                        && (additional_paths_max_modified.is_none()
                        || *modified_res.as_ref().unwrap()
                        > additional_paths_max_modified.unwrap())
                    {
                        additional_paths_max_modified = Some(*modified_res.as_ref().unwrap());

                        if metadata_modified <= additional_paths_max_modified {
                            debug!(
                                "File {:?} is newer than metadata, causing rebuild",
                                entry.path()
                            );
                            break;
                        }
                    } else if modified_res.is_err() {
                        debug!("Cannot get modification time of {:?}", entry.path());
                    }
                }
            }

            if metadata_modified <= additional_paths_max_modified {
                break;
            }
        }
    }

    let first_run = metadata_modified.is_none();
    let skip_rebuild = metadata_modified > script_modified
        && metadata_modified > additional_paths_max_modified
        && build_mode == opt::BuildMode::Default;

    if skip_rebuild {
        debug!("Already compiled, skipping compilation");
    } else {
        // Write files to cache path
        for file in cfg.files.iter() {
            common::write_bytes(
                &script_cache_path,
                &PathBuf::from(&file.path),
                file.content.as_bytes(),
            )?;
        }
        // generate deps file according to language
        if cfg.script_src.ends_with(".go") {
            if cfg.files.iter().find(|f| f.path.ends_with("go.mod")).is_none() {
                let mut deps = vec![];
                for dep in cfg.deps.iter() {
                    deps.push(format!("require {}", dep));
                }
                let mut go_version = "1.22".to_owned();
                if !cfg.language.is_empty() {
                    let parts = cfg.language.trim().split("").collect::<Vec<&str>>();
                    if parts.len() > 1 {
                        go_version = parts[1].to_owned();
                    }
                }
                let go_mod_path = script_cache_path.join("go.mod");
                let go_mod_text = format!("module scriptisto/script\n\ngo {}\n\n{}", go_version, deps.join("\n"));
                common::write_bytes(&script_cache_path, &go_mod_path, go_mod_text.as_bytes())?;
                // modify build command to update go modules
                if let Some(build_cmd) = &cfg.build_cmd {
                    if !build_cmd.contains("go get -u") {
                        cfg.build_cmd = Some(format!("go get -u && {}", build_cmd));
                    }
                }
            }
        }

        run_build_command(
            &cfg,
            &common::script_src_to_absolute(script_path)?,
            &script_cache_path,
            first_run,
            build_mode,
            || {
                if show_logs {
                    Stdio::inherit()
                } else {
                    Stdio::piped()
                }
            },
        )?;
    }

    Ok((cfg, script_cache_path))
}
