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

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Debug, Parser, PartialEq, Eq)]
pub enum CacheCommand {
    /// Shows information about the cache directory for the script.
    Info {
        #[clap(help = "A filename of the script file.")]
        file: PathBuf,
    },
    /// Clean the cache for a particular script. Removes the cache directory. Removes the Docker image/volume if
    /// they exist, but does not prune.
    #[clap(visible_alias = "clear")]
    Clean {
        #[clap(help = "A filename of the script file.")]
        file: PathBuf,
    },
    /// Shows a particular item from "info" by name.
    Get {
        #[clap(help = "An item name, e.g. cache_path.")]
        name: String,
        #[clap(help = "A filename of the script file.")]
        file: PathBuf,
    },
}

#[derive(Debug, Parser, PartialEq, Eq)]
pub enum TemplatesCommand {
    /// Imports a template from file.
    Import {
        #[clap(
            help = "A filename of the script file. Extension will be stripped for the template name."
        )]
        file: PathBuf,
    },
    /// Opens an editor to modify an existing template, nice for quick edits.
    Edit {
        #[clap(help = "A name of the template to edit")]
        template_name: String,
    },
    /// Remove a custom template or reset it to the built-in contents.
    #[clap(name = "rm", visible_aliases = &["remove", "delete"])]
    Remove {
        #[clap(help = "A name of the template to remove")]
        template_name: String,
    },
    /// List all templates.
    #[clap(name = "ls", visible_alias = "list")]
    List {},
}

#[derive(Debug, PartialEq, Eq, Parser, Clone, Default)]
#[clap(rename_all = "snake-case")]
pub enum BuildMode {
    #[default]
    Default,
    Source,
    Full,
}

impl FromStr for BuildMode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        use BuildMode::*;
        Ok(match s {
            "" => Default,
            "source" => Source,
            "full" => Full,
            _ => {
                return Err(anyhow::anyhow!(
                    "Incorrect build mode value. Available values: <unset>, source, full."
                ))
            }
        })
    }
}

#[derive(Debug, Parser, PartialEq, Eq)]
#[clap(
    name = "scriptisto",
    about = "A 'shebang-interpreter' for compiled languages",
    args_conflicts_with_subcommands = true
)]
pub struct Opt {
    /// A path for to a script to run and additional arguments passed to this script. A script path must start with '.' or '/'.
    #[clap(value_name = "SCRIPT")]
    pub command: Vec<String>,

    #[clap(subcommand)]
    pub cmd: Option<Command>,
}

#[derive(Debug, PartialEq, Subcommand, Eq)]
pub enum Command {
    /// Build cache operations.
    Cache {
        #[clap(subcommand)]
        cmd: CacheCommand,
    },
    /// Prints an example starting script in a programming language of your
    /// choice.
    New {
        #[clap(
            help = "If specified, determines a language. Example usage: \"scriptisto new <template_name> | tee new-script\".\nIf not specified, \"new\" lists available templates."
        )]
        template_name: Option<String>,
    },
    /// Manage custom script templates.
    Template {
        #[clap(subcommand)]
        cmd: TemplatesCommand,
    },
    /// Build a script without running.
    Build {
        /// A path to a script to build.
        #[clap()]
        script_src: String,
        /// Build mode. If unset, only builds if necessary. "source" - to rebuild each time. "full" to fully re-fetch Docker image and run `build_once_cmd`.
        #[clap(short, long)]
        build_mode: Option<BuildMode>,
    },
}

pub fn display_help() {
    Opt::parse_from(vec!["", "help"]);
}
