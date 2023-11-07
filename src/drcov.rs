use crate::cli::{Filter, ReplacementFilter};
use crate::util::{parse_capture_group, Hex};
use anyhow::anyhow;
use byteorder::{LittleEndian, ReadBytesExt};
use roaring::RoaringBitmap;
use std::borrow::Cow;
use std::io::{Cursor, Read};
use std::path::Path;

mod constants {
    use lazy_static::lazy_static;
    use regex::bytes::Regex;

    lazy_static! {
        pub static ref DRCOV_VERSION_REGEX: Regex =
            Regex::new(r"DRCOV VERSION: (?P<version>\d+)").unwrap();
        pub static ref DRCOV_FLAVOR_REGEX: Regex =
            Regex::new(r"DRCOV FLAVOR: (?P<flavor>[^\s]+)").unwrap();
        pub static ref DRCOV_MODULE_HEADER_OLD_REGEX: Regex =
            Regex::new(r"Module Table: (?P<count>\d+)").unwrap();
        pub static ref DRCOV_MODULE_HEADER_REGEX: Regex =
            Regex::new(r"Module Table: version (?P<version>\d+), count (?P<count>\d+)").unwrap();
        pub static ref DRCOV_MODULE_V1_REGEX: Regex =
            Regex::new(r"\s*(?P<id>\d+), (?P<size>\d+), (?P<path>[^\s]+)").unwrap();
        pub static ref DRCOV_MODULE_V2_REGEX: Regex =
            Regex::new(r"(?P<id>\d+), 0[xX](?P<base>[[:xdigit:]]+), 0[xX](?P<end>[[:xdigit:]]+), 0[xX](?P<entry>[[:xdigit:]]+), (?P<path>[^\s]+)")
                .unwrap();
        pub static ref DRCOV_MODULE_V3_REGEX: Regex = Regex::new(r"(?P<id>\d+), (?P<containing_id>\d+), 0[xX](?P<base>[[:xdigit:]]+), 0[xX](?P<end>[[:xdigit:]]+), 0[xX](?P<entry>[[:xdigit:]]+), (?P<path>[^\s]+)").unwrap();
        pub static ref DRCOV_MODULE_V4_REGEX: Regex = Regex::new(r"(?P<id>\d+), (?P<containing_id>\d+), 0[xX](?P<base>[[:xdigit:]]+), 0[xX](?P<end>[[:xdigit:]]+), 0[xX](?P<entry>[[:xdigit:]]+), (?P<offset>[[:xdigit:]]+), (?P<path>[^\s]+)").unwrap();
        pub static ref DRCOV_MODULE_V5_REGEX: Regex = Regex::new(r"(?P<id>\d+), (?P<containing_id>\d+), 0[xX](?P<base>[[:xdigit:]]+), 0[xX](?P<end>[[:xdigit:]]+), 0[xX](?P<entry>[[:xdigit:]]+), (?P<offset>[[:xdigit:]]+), 0[xX](?P<preferred_base>[[:xdigit:]]+), (?P<path>[^\s]+)").unwrap();
        pub static ref DRCOV_BB_HEADER_REGEX: Regex = Regex::new(r"BB Table: (?P<count>\d+) bbs").unwrap();
    }
}

#[derive(Debug, Default)]
pub struct Module {
    pub size: usize,
    pub segment_start: usize,
    pub segment_offset: usize,
    pub containing_index: Option<usize>,
    pub path: String,
    pub bb_bitmap: RoaringBitmap,
}

impl Module {
    pub fn from_line_v1(line: &[u8]) -> anyhow::Result<Self> {
        let cap = constants::DRCOV_MODULE_V1_REGEX
            .captures(line)
            .ok_or(anyhow!("Module line is invalid (version = 1)"))?;

        let size = parse_capture_group(&cap, "size")
            .ok_or(anyhow!("Could not find size in module line (version = 1)"))?;

        let path = parse_capture_group(&cap, "path")
            .ok_or(anyhow!("Could not find path in module line (version = 1)"))?;

        Ok(Self {
            size,
            path,
            ..Default::default()
        })
    }

    pub fn from_line_v2(line: &[u8]) -> anyhow::Result<Self> {
        let cap = constants::DRCOV_MODULE_V2_REGEX
            .captures(line)
            .ok_or(anyhow!("Module line is invalid (version = 2)"))?;

        let segment_start: Hex<usize> = parse_capture_group(&cap, "base")
            .ok_or(anyhow!("Could not find base in module line (version = 2)"))?;

        let end: Hex<usize> = parse_capture_group(&cap, "end")
            .ok_or(anyhow!("Could not find end in module line (version = 2)"))?;

        let path = parse_capture_group(&cap, "path")
            .ok_or(anyhow!("Could not find path in module line (version = 2)"))?;

        let size = end.value - segment_start.value;

        Ok(Self {
            size,
            segment_start: segment_start.value,
            path,
            ..Default::default()
        })
    }

    pub fn from_line_v3(line: &[u8]) -> anyhow::Result<Self> {
        let cap = constants::DRCOV_MODULE_V3_REGEX
            .captures(line)
            .ok_or(anyhow!("Module line is invalid (version = 3)"))?;

        let segment_start: Hex<usize> = parse_capture_group(&cap, "base")
            .ok_or(anyhow!("Could not find base in module line (version = 3)"))?;

        let end: Hex<usize> = parse_capture_group(&cap, "end")
            .ok_or(anyhow!("Could not find end in module line (version = 3)"))?;

        let path = parse_capture_group(&cap, "path")
            .ok_or(anyhow!("Could not find path in module line (version = 3)"))?;

        let containing_index = parse_capture_group(&cap, "containing_id").ok_or(anyhow!(
            "Could not find containing id in module line (version = 3)"
        ))?;

        let size = end.value - segment_start.value;

        Ok(Self {
            segment_start: segment_start.value,
            size,
            path,
            containing_index: Some(containing_index),
            ..Default::default()
        })
    }

    pub fn from_line_v4(line: &[u8]) -> anyhow::Result<Self> {
        let cap = constants::DRCOV_MODULE_V4_REGEX
            .captures(line)
            .ok_or(anyhow!("Module line is invalid (version = 4)"))?;

        let segment_start: Hex<usize> = parse_capture_group(&cap, "base")
            .ok_or(anyhow!("Could not find base in module line (version = 4)"))?;

        let end: Hex<usize> = parse_capture_group(&cap, "end")
            .ok_or(anyhow!("Could not find end in module line (version = 4)"))?;

        let path = parse_capture_group(&cap, "path")
            .ok_or(anyhow!("Could not find path in module line (version = 4)"))?;

        let containing_index = parse_capture_group(&cap, "containing_id").ok_or(anyhow!(
            "Could not find containing id in module line (version = 4)"
        ))?;

        let segment_offset: Hex<usize> = parse_capture_group(&cap, "offset").ok_or(anyhow!(
            "Could not find offset in module line (version = 4)"
        ))?;

        let size = end.value - segment_start.value;

        Ok(Self {
            segment_start: segment_start.value,
            segment_offset: segment_offset.value,
            size,
            path,
            containing_index: Some(containing_index),
            ..Default::default()
        })
    }

    pub fn from_line_v5(line: &[u8]) -> anyhow::Result<Self> {
        let cap = constants::DRCOV_MODULE_V5_REGEX
            .captures(line)
            .ok_or(anyhow!("Module line is invalid (version >= 5)"))?;

        let segment_start: Hex<usize> = parse_capture_group(&cap, "base")
            .ok_or(anyhow!("Could not find base in module line (version >= 5)"))?;

        let end: Hex<usize> = parse_capture_group(&cap, "end")
            .ok_or(anyhow!("Could not find end in module line (version >= 5)"))?;

        let path = parse_capture_group(&cap, "path")
            .ok_or(anyhow!("Could not find path in module line (version >= 5)"))?;

        let containing_index = parse_capture_group(&cap, "containing_id").ok_or(anyhow!(
            "Could not find containing id in module line (version >= 5)"
        ))?;

        let segment_offset: Hex<usize> = parse_capture_group(&cap, "offset").ok_or(anyhow!(
            "Could not find offset in module line (version >= 5)"
        ))?;

        let size = end.value - segment_start.value;

        Ok(Self {
            segment_start: segment_start.value,
            segment_offset: segment_offset.value,
            size,
            path,
            containing_index: Some(containing_index),
            ..Default::default()
        })
    }
}

#[derive(Debug)]
pub struct Modules {
    pub version: u32,
    pub table: Vec<Module>,
}

impl Modules {
    pub fn get_coverage_all(&self) -> RoaringBitmap {
        let mut res = RoaringBitmap::new();

        for module in &self.table {
            res.extend(&module.bb_bitmap);
        }

        res
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct BBEntry {
    start: u32,
    size: u16,
    module_id: u16,
}

impl BBEntry {
    pub fn from_reader<R: Read>(reader: &mut R) -> anyhow::Result<Self> {
        let start = reader.read_u32::<LittleEndian>()?;
        let size = reader.read_u16::<LittleEndian>()?;
        let module_id = reader.read_u16::<LittleEndian>()?;

        Ok(Self::new(start, size, module_id))
    }

    pub fn new(start: u32, size: u16, module_id: u16) -> Self {
        Self {
            start,
            size,
            module_id,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DrcovFilters<'r> {
    pub module_filters: &'r [Filter],
    pub module_skip_filters: &'r [Filter],
    pub path_map_filters: &'r [ReplacementFilter],
}

impl DrcovFilters<'_> {
    pub fn matches_any_module_filter(&self, input: &[u8]) -> bool {
        self.module_filters.is_empty()
            || self
                .module_filters
                .iter()
                .any(|filter| filter.matcher.is_match(input))
    }

    pub fn matches_any_module_skip_filter(&self, input: &[u8]) -> bool {
        (!self.module_skip_filters.is_empty())
            && self
                .module_skip_filters
                .iter()
                .any(|filter| filter.matcher.is_match(input))
    }

    pub fn maybe_replace_with_path_map_filter<'d>(&'d self, input: &'d [u8]) -> Cow<[u8]> {
        self.path_map_filters
            .iter()
            .find_map(|filter| {
                filter
                    .matcher
                    .is_match(input)
                    .then(|| filter.matcher.replace(input, filter.replacement.as_bytes()))
            })
            .unwrap_or(Cow::Borrowed(input))
    }
}

#[derive(Debug)]
pub struct Drcov {
    pub version: u32,
    pub flavor: String,
    pub modules: Modules,
}

impl Drcov {
    pub fn from_file<P: AsRef<Path>>(path: P, filters: &DrcovFilters) -> anyhow::Result<Self> {
        fn parse_version<'a, I: Iterator<Item = &'a [u8]>>(
            lines_iter: &mut I,
        ) -> anyhow::Result<u32> {
            log::debug!("Parsing version number");
            let version_line = lines_iter.next().ok_or(anyhow!("Version line missing"))?;

            let cap = constants::DRCOV_VERSION_REGEX
                .captures(version_line)
                .ok_or(anyhow!("Version line does not match the expected format"))?;

            let version = parse_capture_group(&cap, "version")
                .ok_or(anyhow!("Version line does not match the expected format"))?;

            log::debug!("Version number: {version}");

            Ok(version)
        }

        fn parse_flavor<'a, I: Iterator<Item = &'a [u8]>>(
            lines_iter: &mut I,
        ) -> anyhow::Result<String> {
            log::debug!("Parsing flavor");

            let flavor_line = lines_iter.next().ok_or(anyhow!("Flavor line missing"))?;

            let cap = constants::DRCOV_FLAVOR_REGEX
                .captures(flavor_line)
                .ok_or(anyhow!("Flavor line does not match the expected format"))?;

            let flavor = parse_capture_group(&cap, "flavor")
                .ok_or(anyhow!("Flavor line does not match the expected format"))?;

            log::debug!("Flavor: {flavor}");

            Ok(flavor)
        }

        fn parse_num_basic_blocks<'a, I: Iterator<Item = &'a [u8]>>(
            lines_iter: &mut I,
        ) -> anyhow::Result<usize> {
            let bb_header_line = lines_iter
                .next()
                .ok_or(anyhow!("Basic Block header line missing"))?;

            let bb_cap = constants::DRCOV_BB_HEADER_REGEX
                .captures(bb_header_line)
                .ok_or(anyhow!("Invalid Basic Block header line"))?;

            let num_bb = parse_capture_group(&bb_cap, "count")
                .ok_or(anyhow!("Coud not parse number of basic blocks"))?;

            Ok(num_bb)
        }

        fn parse_basic_blocks(
            bb_data: &[u8],
            num_bb: usize,
            modules: &mut Vec<Module>,
        ) -> anyhow::Result<()> {
            let mut cursor = Cursor::new(bb_data);

            let num_modules = modules.len();

            for _ in 0..num_bb {
                let bb = BBEntry::from_reader(&mut cursor)?;
                if (bb.module_id as usize) < num_modules {
                    let module = &mut modules[bb.module_id as usize];

                    if module.size <= (bb.start + bb.size as u32) as usize {
                        continue;
                    }

                    let addr_start = bb.start;
                    let addr_end = bb.start + bb.size as u32 - 1;

                    module.bb_bitmap.insert_range(addr_start..addr_end);
                }
            }

            Ok(())
        }

        fn parse_modules<'a, I: Iterator<Item = &'a [u8]>>(
            lines_iter: &mut I,
            filters: &DrcovFilters,
        ) -> anyhow::Result<Modules> {
            log::debug!("Parsing modules");

            let header_line = lines_iter
                .next()
                .ok_or(anyhow!("Modules header line missing"))?;

            let invalid_module_header_line_err =
                "Modules header line does not match the expected format";

            let (version, num_modules) = if let Some(cap) =
                constants::DRCOV_MODULE_HEADER_OLD_REGEX.captures(header_line)
            {
                let version = 1u32;

                let count = parse_capture_group(&cap, "count")
                    .ok_or(anyhow!(invalid_module_header_line_err))?;

                (version, count)
            } else if let Some(cap) = constants::DRCOV_MODULE_HEADER_REGEX.captures(header_line) {
                let version = parse_capture_group(&cap, "version")
                    .ok_or(anyhow!(invalid_module_header_line_err))?;

                let count = parse_capture_group(&cap, "count")
                    .ok_or(anyhow!(invalid_module_header_line_err))?;

                lines_iter.next();

                (version, count)
            } else {
                anyhow::bail!(invalid_module_header_line_err)
            };

            let parser = match version {
                1 => Module::from_line_v1,
                2 => Module::from_line_v2,
                3 => Module::from_line_v3,
                4 => Module::from_line_v4,
                _ => Module::from_line_v5,
            };

            let mut table = Vec::with_capacity(num_modules);

            for _ in 0..num_modules {
                let line = lines_iter
                    .next()
                    .map(|line| filters.maybe_replace_with_path_map_filter(line))
                    .ok_or(anyhow!("Invalid module table (lines missing)"))?;

                if !filters.matches_any_module_filter(line.as_ref())
                    || filters.matches_any_module_skip_filter(line.as_ref())
                {
                    continue;
                }

                let module = parser(line.as_ref())?;

                table.push(module);
            }

            // Resolve offsets based on containing_index
            if version >= 3 {
                for i in 0..table.len() {
                    if let Some(containing_index) = table[i].containing_index {
                        if containing_index != i {
                            assert!(i < containing_index);
                            table[i].segment_offset =
                                table[i].segment_start - table[containing_index].segment_start;
                        }
                    }
                }
            }

            log::debug!("Modules version: {version}, Number of modules: {num_modules}");

            Ok(Modules { version, table })
        }

        log::info!("Loading drcov file: {}", path.as_ref().display());
        let mut cursor: usize = 0;
        let contents = std::fs::read(path)?;

        let mut lines_iter = contents
            .as_slice()
            .split(|b| *b == b'\n')
            .filter(|line| !line.is_empty())
            .inspect(|v| cursor += v.len() + 1);

        let version = parse_version(&mut lines_iter)?;
        let flavor = parse_flavor(&mut lines_iter)?;
        let mut modules = parse_modules(&mut lines_iter, &filters)?;
        let num_bb = parse_num_basic_blocks(&mut lines_iter)?;

        log::debug!("Number of Basic Blocks: {num_bb}");

        drop(lines_iter);

        let bb_data = &contents[cursor..];

        parse_basic_blocks(bb_data, num_bb, &mut modules.table)?;

        log::debug!("Modules parsed: {:#?}", modules.table);
        log::info!("Drcov file loaded");

        Ok(Self {
            version,
            flavor,
            modules,
        })
    }
}
