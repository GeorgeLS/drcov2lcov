mod cli;
mod drcov;
mod dwarf;
mod util;

use crate::cli::CliOptions;
use crate::drcov::Drcov;
use crate::dwarf::{gather_line_info, LineInfo};
use itertools::Itertools;
use std::collections::HashMap;
use std::fmt::Write;

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

    let mut previous_coverages = options.reduce_set_path.is_some().then(|| Vec::new());
    let mut reduced_input_set = options.reduce_set_path.is_some().then(|| Vec::new());

    for input_file in &input_files {
        match Drcov::from_file(input_file.as_path(), &drcov_filters) {
            Ok(drcov) => {
                let info = gather_line_info(&drcov.modules, &line_info_filters);
                line_info.extend(info);

                if options.reduce_set_path.is_some() {
                    // Safety: We can unwrap here since we know these values have been set
                    let previous_coverages = previous_coverages.as_mut().unwrap();
                    let reduced_input_set = reduced_input_set.as_mut().unwrap();

                    let modules_coverage = drcov.modules.get_coverage_all();

                    if !previous_coverages
                        .iter()
                        .any(|coverage| *coverage == modules_coverage)
                    {
                        reduced_input_set.push(input_file.to_string_lossy().to_string());
                        previous_coverages.push(modules_coverage);
                    }
                }
            }
            Err(e) => {
                log::warn!("Could not parse '{}' as a drcov file. Skipping from line coverage analysis. Reason: {e}", input_file.display())
            }
        }
    }

    if let Some(reduce_set_path) = options.reduce_set_path {
        // Safety: We can unwrap here since we know this value has been set
        let reduced_input_set = reduced_input_set.unwrap();
        std::fs::write(reduce_set_path, reduced_input_set.join("\n"))?;
    }

    write_lcov_output(&options.output, &line_info)?;

    Ok(())
}
