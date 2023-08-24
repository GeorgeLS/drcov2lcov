mod drcov;
mod util;

use crate::drcov::Drcov;
use clap::Parser;
use simple_logger::SimpleLogger;
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
}

fn main() -> anyhow::Result<()> {
    SimpleLogger::new().init()?;

    let options = CliOptions::parse_and_validate()?;

    let _drcov = Drcov::from_file(&options.input)?;

    Ok(())
}
