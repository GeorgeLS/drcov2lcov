use crate::cli::Filter;
use crate::drcov::{Module, Modules};
use gimli::{Dwarf, DwarfSections, LineProgramHeader, LineRow, Reader, Unit};
use itertools::Itertools;
use object::{Object, ObjectSection, ObjectSegment, SegmentFlags};
use ouroboros::self_referencing;
use std::borrow::Cow;
use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

mod constants {

    pub const UNKNOWN_MODULE: &str = "<unknown>";
}

#[self_referencing]
#[derive(Debug)]
struct ObjectFile {
    mmap: memmap2::Mmap,
    #[borrows(mmap)]
    #[not_covariant]
    pub object: object::File<'this>,
}

impl ObjectFile {
    pub fn load_base(&self) -> u64 {
        self.with_object(|obj| obj)
            .segments()
            .filter_map(|s| {
                if let SegmentFlags::Elf { p_flags } = s.flags() {
                    (p_flags & object::elf::PT_LOAD != 0).then_some(s.address() - s.file_range().0)
                } else {
                    None
                }
            })
            .min()
            .unwrap_or_default()
    }
}

impl ObjectFile {
    pub fn from_path(path: &str) -> anyhow::Result<Self> {
        let file = std::fs::File::open(path)?;
        let mmap = unsafe { memmap2::Mmap::map(&file)? };
        let res = ObjectFileBuilder {
            mmap,
            object_builder: |mmap| object::File::parse(&**mmap).unwrap(),
        }
        .build();

        Ok(res)
    }
}

/*
 * Gdb's search algorithm for finding debug info files is documented here:
 *  http://sourceware.org/gdb/onlinedocs/gdb/Separate-Debug-Files.html
 */
fn follow_debug_link(object: &object::File) -> Option<String> {
    let Ok(Some((debug_link, _))) = object.gnu_debuglink() else {
        return None;
    };

    let debug_link = String::from_utf8_lossy(debug_link);

    const DEBUG_PATH: &str = "/usr/lib/debug";
    let debug_link_path = PathBuf::from(debug_link.as_ref());

    if debug_link_path.is_absolute() && debug_link_path.exists() {
        return Some(debug_link_path.to_string_lossy().to_string());
    }

    // 1. Check /usr/lib/debug/.build-id/xx/$debuglink
    if let Ok(Some(build_id)) = object.build_id() {
        if build_id[0] != 0 {
            let result_path = format!(
                "{DEBUG_PATH}/{}/{}/{}",
                build_id[0],
                build_id[1],
                debug_link_path.display()
            );

            let result_path = Path::new(&result_path);
            if result_path.exists() {
                return Some(result_path.to_string_lossy().to_string());
            }
        }
    }

    let mod_dir = debug_link_path.parent()?;

    // 2. Check $mod_dir/$debuglink
    let mut mod_path = mod_dir.to_path_buf();
    mod_path.push(debug_link.as_ref());

    if mod_path.exists() {
        if let Some((mod_path_meta, debug_link_meta)) = mod_path
            .metadata()
            .ok()
            .zip(debug_link_path.metadata().ok())
        {
            if mod_path_meta.ino() != debug_link_meta.ino() {
                return Some(mod_path.to_string_lossy().to_string());
            }
        }
    }

    // 3. Check $mod_dir/.debug/$debuglink
    let mut mod_path = mod_dir.to_path_buf();
    mod_path.push(".debug");
    mod_path.push(debug_link.as_ref());

    if mod_path.exists() {
        return Some(mod_path.to_string_lossy().to_string());
    }

    // 4. Check /usr/lib/debug/$mod_dir/$debuglink
    let mut mod_path = PathBuf::from(DEBUG_PATH);
    mod_path.push(mod_dir);
    mod_path.push(debug_link.as_ref());

    if mod_path.exists() {
        return Some(mod_path.to_string_lossy().to_string());
    }

    None
}

fn get_module_object_with_debug_info(module: &Module) -> anyhow::Result<Option<ObjectFile>> {
    let mut stack = Vec::new();
    stack.push(ObjectFile::from_path(&module.path)?);

    while let Some(module_object) = stack.pop() {
        let object = module_object.with_object(|obj| obj);

        if let Some(debug_link_module_path) = follow_debug_link(object) {
            stack.push(ObjectFile::from_path(&debug_link_module_path)?);
        } else if object.has_debug_symbols() {
            return Ok(Some(module_object));
        }
    }

    Ok(None)
}

#[derive(Debug, Clone)]
pub struct LineInfoFilters<'r> {
    pub src_filters: &'r [Filter],
    pub src_skip_filters: &'r [Filter],
}

impl LineInfoFilters<'_> {
    pub fn matches_any_source_filter(&self, source: Option<&String>) -> bool {
        source.is_some_and(|source| {
            self.src_filters.is_empty()
                || self
                    .src_filters
                    .iter()
                    .any(|filter| filter.matcher.is_match(source.as_bytes()))
        })
    }

    pub fn matches_any_source_skip_filter(&self, source: Option<&String>) -> bool {
        source.is_some_and(|source| {
            (!self.src_skip_filters.is_empty())
                && self
                    .src_skip_filters
                    .iter()
                    .any(|filter| filter.matcher.is_match(source.as_bytes()))
        })
    }
}

#[derive(Debug)]
pub struct LineInfo {
    pub line: u64,
    pub executed: bool,
}

fn get_program_file<R: Reader>(
    dwarf: &Dwarf<R>,
    unit: &Unit<R>,
    header: &LineProgramHeader<R>,
    row: &LineRow,
) -> Option<String> {
    if let Some(file) = row.file(header) {
        let mut path = PathBuf::new();

        if let Some(dir) = file.directory(header) {
            path.push(
                dwarf
                    .attr_string(unit, dir)
                    .ok()?
                    .to_string_lossy()
                    .ok()?
                    .as_ref(),
            );
        }

        path.push(
            dwarf
                .attr_string(unit, file.path_name())
                .ok()?
                .to_string_lossy()
                .ok()?
                .as_ref(),
        );

        Some(path.to_string_lossy().to_string())
    } else {
        None
    }
}

fn gather_object_file_debug_info(
    module: &Module,
    object_file: &ObjectFile,
    line_table: &mut HashMap<String, Vec<LineInfo>>,
    filters: &LineInfoFilters,
) -> anyhow::Result<()> {
    let object = object_file.with_object(|obj| obj);
    let load_base = object_file.load_base();

    let endian = if object.is_little_endian() {
        gimli::RunTimeEndian::Little
    } else {
        gimli::RunTimeEndian::Big
    };

    let load_section = |id: gimli::SectionId| -> Result<Cow<[u8]>, gimli::Error> {
        match object.section_by_name(id.name()) {
            Some(ref section) => Ok(section
                .uncompressed_data()
                .unwrap_or(Cow::Borrowed(&[][..]))),
            None => Ok(Cow::Borrowed(&[][..])),
        }
    };

    let borrow_section=
        |section| gimli::EndianSlice::new(Cow::as_ref(section), endian);

    let dwarf = DwarfSections::load(&load_section)?;
    let dwarf = dwarf.borrow(&borrow_section);

    let mut units_iter = dwarf.units();

    while let Some(header) = units_iter.next()? {
        let unit = dwarf.unit(header)?;
        let unit = unit.unit_ref(&dwarf);

        if let Some(program) = unit.line_program.clone() {
            let mut rows = program.rows();

            while let Some((header, row)) = rows.next_row()? {
                let program_file = get_program_file(&dwarf, &unit, header, row);

                if !filters.matches_any_source_filter(program_file.as_ref())
                    || filters.matches_any_source_skip_filter(program_file.as_ref())
                {
                    continue;
                }

                let Some(line) = row.line().map(|v| v.get()) else {
                    continue;
                };
                let addr = row.address() - load_base - module.segment_offset as u64;

                if addr > u32::MAX as u64 || module.size <= addr as usize {
                    continue;
                }

                let executed = module.bb_bitmap.contains(addr as u32);
                let line_info = LineInfo { line, executed };

                line_table
                    .entry(program_file.as_ref().unwrap().to_string())
                    .or_default()
                    .push(line_info);
            }
        }
    }

    Ok(())
}

fn coalesce_line_info(line_table: &mut HashMap<String, Vec<LineInfo>>) {
    let mut line_map = HashMap::new();
    for info in line_table.values_mut() {
        for line_info in info.drain(..) {
            *line_map.entry(line_info.line).or_default() |= line_info.executed;
        }
        for (line, executed) in line_map
            .iter()
            .map(|(l, e)| (*l, *e))
            .sorted_by(|(l1, _), (l2, _)| l1.cmp(l2))
        {
            info.push(LineInfo { line, executed })
        }

        line_map.clear();
    }
}

pub fn gather_line_info(
    modules: &Modules,
    filters: &LineInfoFilters,
) -> HashMap<String, Vec<LineInfo>> {
    let mut line_table = HashMap::new();

    for module in &modules.table {
        if module.path == constants::UNKNOWN_MODULE {
            continue;
        }

        log::info!("Gathering debug information about module {}", module.path);

        match get_module_object_with_debug_info(module) {
            Ok(Some(object_file)) => {
                match gather_object_file_debug_info(module, &object_file, &mut line_table, filters) {
                    Err(err) => log::error!("An error occurred while gathering debug info for {}. Info: {}", module.path, err),
                    _ => {
                        log::info!("Gathered debug information about module {}", module.path);
                    }
                }
            }
            Ok(None) => log::warn!("Could not find debug info for {}", module.path),
            Err(err) => log::error!("An error occurred while trying to get determine whether {} has debug info. Info: {}", module.path, err),
        }
    }

    coalesce_line_info(&mut line_table);

    line_table
}
