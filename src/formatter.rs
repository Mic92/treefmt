//! Utilities for the formatters themselves.
use crate::config::FmtConfig;
use crate::{expand_if_path, expand_path};
use anyhow::{anyhow, Result};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use log::debug;
use path_clean::PathClean;
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    fmt,
    path::Path,
    path::PathBuf,
    process::{Command, Output},
};
use which::which;

/// newtype for the formatter name
#[derive(Debug, Default, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FormatterName(String);

impl Serialize for FormatterName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

// All of this is for the serde deserialized. Maybe there is a more elegant way to do this?
struct FormatterNameVisitor;

impl<'de> Visitor<'de> for FormatterNameVisitor {
    type Value = FormatterName;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a string")
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(FormatterName(value))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(FormatterName(value.to_string()))
    }
}

impl<'de> Deserialize<'de> for FormatterName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_string(FormatterNameVisitor)
    }
}

/// Display formatters as "#name"
impl fmt::Display for FormatterName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// An instance of a formatter respecting the spec.
#[derive(Debug, Clone)]
pub struct Formatter {
    /// Name of the formatter for display purposes
    pub name: FormatterName,
    /// Command formatter to run
    pub command: PathBuf,
    /// Argument for formatter
    pub options: Vec<String>,
    /// Working directory for formatter
    pub work_dir: PathBuf,
    /// File or Folder that is included to be formatted
    pub includes: GlobSet,
    /// File or Folder that is excluded to be formatted
    pub excludes: GlobSet,
}

impl Formatter {
    /// Run the formatter on the given paths
    // TODO: handle E2BIG
    pub fn fmt(&self, paths: &[PathBuf]) -> Vec<Box<dyn Fn(()) -> anyhow::Result<()>>> {
        // FIXME: make chunks configurable
        paths.chunks(1020).map(|paths|
            Box::new(move || -> anyhow::Result<()> {
                let mut cmd_arg = Command::new(&self.command);
                // Set the command to run under its working directory.
                cmd_arg.current_dir(&self.work_dir);
                // Append the default options to the command.
                cmd_arg.args(&self.options);
                // Append all of the file paths to format.
                cmd_arg.args(paths);
                // And run
                let output = cmd_arg.output()?;
                if output.status > 0 {
                    anyhow!("exited with {}", output.status);
                }

                // TODO: print stdout and stderr

                Ok(())
            })
        ).collect()
    }

    /// Returns the formatter if the path matches the formatter rules.
    pub fn is_match<T: AsRef<Path>>(&self, path: T) -> bool {
        let path = path.as_ref();
        assert!(path.is_absolute());
        // Ignore any paths that are outside of the formatter work_dir
        if !path.starts_with(&self.work_dir) {
            return false;
        }
        // Ignore if any of the excludes match
        if self.excludes.is_match(path) {
            return false;
        }
        // Return true if any of the includes match
        if !self.includes.is_match(path) {
            return false;
        }
        true
    }

    /// Load the formatter matcher from a config fragment
    pub fn from_config(tree_root: &Path, name: &str, cfg: &FmtConfig) -> Result<Self> {
        let name = FormatterName(name.to_string());
        // Expand the work_dir to an absolute path, using the project root as a reference.
        let work_dir = expand_path(&cfg.work_dir, tree_root);
        // Resolve the path to the binary
        let command = which(&cfg.command)?.clean();
        debug!("Found {} at {}", cfg.command.display(), command.display());
        assert!(command.is_absolute());

        // Build the include and exclude globs
        if cfg.includes.is_empty() {
            return Err(anyhow!("{} doesn't have any includes", name));
        }
        let includes = patterns_to_glob_set(tree_root, &cfg.includes)?;
        let excludes = patterns_to_glob_set(tree_root, &cfg.excludes)?;

        Ok(Self {
            name,
            command,
            options: cfg.options.clone(),
            work_dir,
            includes,
            excludes,
        })
    }
}

/// Display formatters as "#name"
impl fmt::Display for Formatter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.name.0)
    }
}

/// Small utility to convert config globs to a GlobSet.
fn patterns_to_glob_set(tree_root: &Path, patterns: &[String]) -> Result<GlobSet> {
    let mut sum = GlobSetBuilder::new();
    for pattern in patterns {
        let pattern = expand_if_path(pattern.to_string(), &tree_root);
        let glob = GlobBuilder::new(&pattern).build()?;
        sum.add(glob);
    }
    Ok(sum.build()?)
}
