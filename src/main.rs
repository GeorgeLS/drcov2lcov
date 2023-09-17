mod drcov;
mod dwarf;
mod util;

use crate::drcov::{Drcov, DrcovFilters};
use crate::dwarf::{gather_line_info, LineInfo, LineInfoFilters};
use clap::Parser;
use itertools::Itertools;
use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::path::{Path, PathBuf};

mod constants {
    use lazy_static::lazy_static;
    use regex::Regex;

    pub const DEFAULT_OUTPUT_FILE: &str = "coverage.info";
    lazy_static! {
        pub static ref DRCOV_LOG_FILE_REGEX: Regex = Regex::new(r"(dr|bb)cov\..*\.?log").unwrap();
    }
}

fn default_output_file() -> String {
    let mut path = std::env::current_dir().unwrap();
    path.push(constants::DEFAULT_OUTPUT_FILE);
    path.to_string_lossy().to_string()
}

#[derive(Debug, Parser)]
struct CliOptions {
    #[clap(short, long, required_unless_present_any(["directory", "list"]), help = "The path to the input file")]
    pub input: Option<String>,
    #[clap(short, long, required_unless_present_any(["input", "list"]), help = "Directory with drcov.*.log files to process")]
    pub directory: Option<String>,
    #[clap(short, long, required_unless_present_any(["input", "directory"]), help = "Text file listing log files to process")]
    pub list: Option<String>,
    #[clap(short, long, default_value_t = default_output_file(), help = "The path to the output file")]
    pub output: String,
    #[clap(
        long,
        help = "Only include coverage for modules that match the given regular expression"
    )]
    pub module_filter: Option<String>,
    #[clap(
        long,
        help = "Skip coverage for the modules that match the given regular expression"
    )]
    pub module_skip_filter: Option<String>,
    #[clap(
        long,
        help = "Only include coverage for source files that match the given regular expression"
    )]
    pub source_filter: Option<String>,
    #[clap(
        long,
        help = "Skip coverage for source files that match the given regular expression"
    )]
    pub source_skip_filter: Option<String>,
}

impl CliOptions {
    pub fn parse_and_validate() -> anyhow::Result<Self> {
        let self_ = Self::parse();

        if let Some(input_path) = self_.input.as_ref().map(Path::new) {
            if !input_path.exists() {
                anyhow::bail!("Input path '{}' does not exist", input_path.display());
            }

            if !input_path.is_file() {
                anyhow::bail!("Input path '{}' is not a file", input_path.display());
            }
        }

        if let Some(directory) = self_.directory.as_ref().map(Path::new) {
            if !directory.exists() {
                anyhow::bail!("Directory '{}' does not exist", directory.display());
            }

            if !directory.is_dir() {
                anyhow::bail!(
                    "Given directory '{}' is not a directory",
                    directory.display()
                );
            }
        }

        if let Some(list_file) = self_.list.as_ref().map(Path::new) {
            if !list_file.exists() {
                anyhow::bail!("List file path '{}' does not exist", list_file.display());
            }

            if !list_file.is_file() {
                anyhow::bail!("List file path '{}' is not a file", list_file.display());
            }
        }

        let output_path = Path::new(&self_.output);

        if output_path.parent().is_some_and(|parent| !parent.is_dir()) {
            anyhow::bail!(
                "Target output path '{}' does not point to a valid directory",
                self_.output
            );
        }

        Ok(self_)
    }

    pub fn get_input_files(&self) -> Vec<PathBuf> {
        let mut unique_files = HashSet::new();

        if let Some(input) = self.input.as_ref().map(PathBuf::from) {
            let input = input.canonicalize().unwrap_or(input);
            unique_files.insert(input);
        }

        if let Some(list_file) = &self.list {
            if let Ok(contents) = std::fs::read_to_string(list_file) {
                for line in contents.lines() {
                    let path = PathBuf::from(line);
                    let path = path.canonicalize().unwrap_or(path);
                    unique_files.insert(path);
                }
            }
        }

        if let Some(directory) = self.directory.as_ref().map(Path::new) {
            if let Ok(read_dir) = directory.read_dir() {
                for entry in read_dir.flatten() {
                    if entry.file_type().is_ok_and(|file_type| file_type.is_file())
                        && constants::DRCOV_LOG_FILE_REGEX
                            .is_match(&entry.file_name().to_string_lossy())
                    {
                        let path = entry.path();
                        let path = path.canonicalize().unwrap_or(path);
                        unique_files.insert(path);
                    }
                }
            }
        }

        unique_files.into_iter().collect()
    }

    pub fn get_drcov_filters(&self) -> DrcovFilters {
        let module_filter = self
            .module_filter
            .clone()
            .and_then(|filter| regex::bytes::Regex::new(&filter).ok());

        let module_skip_filter = self
            .module_skip_filter
            .clone()
            .and_then(|filter| regex::bytes::Regex::new(&filter).ok());

        DrcovFilters {
            module_filter,
            module_skip_filter,
        }
    }

    pub fn get_line_info_filters(&self) -> LineInfoFilters {
        let src_filter = self
            .source_filter
            .clone()
            .and_then(|filter| regex::Regex::new(&filter).ok());

        let src_skip_filter = self
            .source_skip_filter
            .clone()
            .and_then(|filter| regex::Regex::new(&filter).ok());

        LineInfoFilters {
            src_filter,
            src_skip_filter,
        }
    }
}

fn write_lcov_output(path: &str, line_info: &HashMap<String, Vec<LineInfo>>) -> anyhow::Result<()> {
    let mut res = String::new();
    for file in line_info.keys().sorted() {
        let _ = writeln!(res, "SF:{file}");
        for info in &line_info[file] {
            let _ = writeln!(
                res,
                "DA:{},{}",
                info.line,
                if info.executed { 1 } else { 0 }
            );
        }
        let _ = writeln!(res, "end_of_record");
    }

    std::fs::write(path, res)?;

    Ok(())
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let options = CliOptions::parse_and_validate()?;

    let input_files = options.get_input_files();

    let drcov_filters = options.get_drcov_filters();

    let line_info_filters = options.get_line_info_filters();

    let mut line_info = HashMap::new();

    for input_file in input_files {
        match Drcov::from_file(input_file.as_path(), &drcov_filters) {
            Ok(drcov) => {
                let info = gather_line_info(&drcov.modules, &line_info_filters);
                line_info.extend(info);
            }
            Err(e) => {
                log::warn!("Could not parse '{}' as a drcov file. Skipping from line coverage analysis. Reason: {e}", input_file.display())
            }
        }
    }

    write_lcov_output(&options.output, &line_info)?;

    Ok(())
}
