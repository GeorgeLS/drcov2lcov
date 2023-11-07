use crate::drcov::DrcovFilters;
use crate::dwarf::LineInfoFilters;
use clap::Parser;
use regex::bytes::Regex;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::str::FromStr;

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

#[derive(Debug, Clone)]
pub struct Filter {
    pub matcher: Regex,
}

impl FromStr for Filter {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let filter = Regex::new(s)
            .map_err(|_| format!("Could not create a regular expression from '{s}'"))?;

        Ok(Self { matcher: filter })
    }
}

#[derive(Debug, Clone)]
pub struct ReplacementFilter {
    pub matcher: Regex,
    pub replacement: String,
}

impl FromStr for ReplacementFilter {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let pos = s
            .find(':')
            .ok_or_else(|| format!("Invalid path_map argument: no '=' found in '{s}'"))?;

        let matcher = Regex::new(&s[..pos])
            .map_err(|_| format!("Could not create a regular expression from '{}'", &s[..pos]))?;

        let res = Self {
            matcher,
            replacement: s[pos + 1..].to_string(),
        };

        Ok(res)
    }
}

#[derive(Debug, Parser)]
pub struct CliOptions {
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
        value_parser = clap::value_parser!(Filter),
        help = "Only include coverage for modules that match the given regular expressions"
    )]
    pub module_filters: Vec<Filter>,
    #[clap(
        long,
        value_parser = clap::value_parser!(Filter),
        help = "Skip coverage for the modules that match the given regular expressions"
    )]
    pub module_skip_filters: Vec<Filter>,
    #[clap(
        long,
        value_parser = clap::value_parser!(Filter),
        help = "Only include coverage for source files that match the given regular expressions"
    )]
    pub source_filters: Vec<Filter>,
    #[clap(
        long,
        value_parser = clap::value_parser!(Filter),
        help = "Skip coverage for source files that match the given regular expressions"
    )]
    pub source_skip_filters: Vec<Filter>,
    #[clap(
        short,
        long,
        value_parser = clap::value_parser!(ReplacementFilter),
        help = "Takes two values: the first specifies the library path to look for in each drcov log file and the second specifies the path to replace it with before looking for debug information for that library. You can provide this option multiple times for different mappings. Values should be separated by a colon (:)"
    )]
    pub path_map_filters: Vec<ReplacementFilter>,
    #[clap(
        short,
        long,
        help = "Reduce the set of drov files from the input to a smaller set of drcov files containing the same coverage information and store the input files into the given path"
    )]
    pub reduce_set_path: Option<String>,
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
        DrcovFilters {
            module_filters: self.module_filters.as_slice(),
            module_skip_filters: self.module_skip_filters.as_slice(),
            path_map_filters: self.path_map_filters.as_slice(),
        }
    }

    pub fn get_line_info_filters(&self) -> LineInfoFilters {
        LineInfoFilters {
            src_filters: self.source_filters.as_slice(),
            src_skip_filters: self.source_skip_filters.as_slice(),
        }
    }
}
