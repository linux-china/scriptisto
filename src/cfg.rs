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

use anyhow::{Context, Result};
use log::debug;
use serde_derive::Deserialize;
use std::cmp::min;
use std::io::{BufRead, BufReader};

#[derive(Deserialize, Debug)]
pub struct BuildSpec {
    pub script_src: String,
    pub build_cmd: Option<String>,
    pub build_once_cmd: Option<String>,
    #[serde(default = "default_target_bin")]
    pub target_bin: String,
    pub target_interpreter: Option<String>,
    #[serde(default)]
    pub replace_shebang_with: String,
    #[serde(default)]
    pub files: Vec<File>,
    #[serde(default)]
    pub deps: Vec<String>,
    #[serde(default)]
    pub docker_build: Option<DockerBuild>,
    #[serde(default)]
    pub extra_src_paths: Vec<String>, // paths to directory/file, no wildcards supported
    #[serde(default)]
    pub build_in_script_dir: bool, // use script directory as working directory of build, not the cache directory (non-Docker build only)
}

fn default_target_bin() -> String {
    "./script".into()
}

#[derive(Deserialize, Debug)]
pub struct File {
    pub path: String,
    pub content: String,
}

#[derive(Deserialize, Debug)]
pub struct DockerBuild {
    pub dockerfile: Option<String>,
    pub src_mount_dir: Option<String>,
    #[serde(default)]
    pub extra_args: Vec<String>,
}

#[derive(Clone, Debug)]
enum ParserState {
    ScriptSource,
    ConfigSource { prefix_len: usize },
}

impl BuildSpec {
    pub fn new(script_body: &[u8]) -> Result<Self> {
        let mut script_src = Vec::new();
        let reader = BufReader::new(script_body);

        use ParserState::*;
        let mut state = ParserState::ScriptSource;

        let mut cfg_src = vec![];

        for (line_num, line) in reader.lines().enumerate() {
            let mut line = line.context(format!("Cannot parse script line: {}", line_num))?;
            script_src.push(line.clone());
            state = match state {
                ScriptSource => {
                    let sb_start = line.find("scriptisto-begin");
                    if let Some(pos) = sb_start {
                        ConfigSource { prefix_len: pos }
                    } else {
                        state
                    }
                }
                ConfigSource { prefix_len } => {
                    line.drain(..min(prefix_len, line.len()));
                    if line.starts_with("scriptisto-end") {
                        ScriptSource
                    } else {
                        cfg_src.push(line);
                        state
                    }
                }
            };
        }

        let mut build_spec: BuildSpec = serde_yaml::from_str(&cfg_src.join("\n"))
            .context(format!("Cannot parse config YAML: \n{:#?}", cfg_src))?;

        // check to replace scriptisto shebang or not
        if !script_src.is_empty() {
            let first_line = &script_src[0];
            if first_line.starts_with("#!") && first_line.contains("scriptisto") {
                script_src[0] = build_spec.replace_shebang_with.clone();
            }
        }

        build_spec.files.push(File {
            path: build_spec.script_src.clone(),
            content: script_src.join("\n"),
        });

        debug!("BuildSpec parsed: {:#?}", build_spec);

        Ok(build_spec)
    }
}
