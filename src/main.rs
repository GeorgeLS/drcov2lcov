mod drcov;
mod dwarf;
mod util;

use crate::drcov::{Drcov, DrcovFilters};
use crate::dwarf::{gather_line_info, LineInfo};
use clap::Parser;
use itertools::Itertools;
use std::collections::HashMap;
use std::fmt::Write;
use std::path::Path;

mod constants {
    pub const DEFAULT_OUTPUT_FILE: &str = "coverage.info";
}

fn default_output_file() -> String {
    let mut path = std::env::current_dir().unwrap();
    path.push(constants::DEFAULT_OUTPUT_FILE);
    path.to_string_lossy().to_string()
}

#[derive(Debug, Parser)]
struct CliOptions {
    #[clap(short, long, help = "The path to the input file")]
    pub input: String,
    #[clap(short, long, default_value_t = default_output_file(), help = "The path to the output file")]
    pub output: String,
    #[clap(short, long, help = "Only include coverage for this library")]
    pub module_filter: Option<String>,
}

impl CliOptions {
    pub fn parse_and_validate() -> anyhow::Result<Self> {
        let self_ = Self::parse();

        let input_path = Path::new(&self_.input);

        if !input_path.exists() {
            anyhow::bail!("Input path '{}' does not exist", self_.input);
        }

        if !input_path.is_file() {
            anyhow::bail!("Input path '{}' is not a file", self_.input);
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

    pub fn get_drcov_filters(&self) -> DrcovFilters {
        DrcovFilters {
            module_filter: self.module_filter.clone(),
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

    let drcov_filters = options.get_drcov_filters();

    let drcov = Drcov::from_file(&options.input, drcov_filters)?;

    let line_info = gather_line_info(&drcov.modules);

    write_lcov_output(&options.output, &line_info)?;

    Ok(())
}
