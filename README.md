# drcov2lcov

drcov2lcov is an implementation of [DynamoRIO's](https://dynamorio.org/page_drcov.html) drcov2lcov in Rust.

Currently supported features:

| Feature                                                                                                                                                         | DynamoRIO's flag |
|-----------------------------------------------------------------------------------------------------------------------------------------------------------------|------------------|
| Specify a single input file using `-i` or `--input` option                                                                                                      | `-input`         |
| Specify an output file to store line coverage information. Default: `<current_directory>/coverage.info`                                                         | `-output`        |
| Filter specific modules from the `drcov` file. This allows generating line coverage for only the modules matching the given regular expression. Default: `None` | `-mod_filter`    |

Unsupported features:

| Feature                                                                                                                  | DynamoRIO's flag   |
|--------------------------------------------------------------------------------------------------------------------------|--------------------|
| Specify a directory from which all `drcov.*.log` files will be processed                                                 | `-dir`             |
| Specify a file which contains a list of `drcov` files to process                                                         | `-list`            |
| Specify a test function regular expression in order to generate test coverage information in the output file             | `-test_pattern`    |
| Filter out specific modules from line coverage collection                                                                | `-mod_skip_filter` |
| Filter specific source code paths from each module for line coverage collection                                          | `-src_filter`      |
| Filter out specific source code paths from each module for line coverage collection                                      | `-src_skip_filter` |
| Reduce the set of `drov` files from the input to a smaller set of `drcov` files containing the same coverage information | `-reduce_set`      |
| Replace a module path with another one before gathering debug info for that module                                       | `-pathmap`         |

Adding support for the above features is about 2 hours of work so these will be added in a weekend when I have some time.\
The main purpose of this tool however has been implemented, which is to generate line coverage from `drcov` files.\
Also, all `drcov` file version formats are supported (1-5).

## Extra features vs DynamoRIO's drcov2lcov

This implementation of `drcov2lcov` supports generating line coverage for executables/libraries that have been compiled with Dwarf v5 symbols (this is the default for latest compilers)\
as well as for executables/libraries that have been compiled with compressed Dwarf data.

## Usage

In order to generate line coverage from a `drcov` file you can just run

```bash
drcov2lcov --input <input_file>
```

This will generate a `coverage.info` to the current working directory containing the line coverage.\
If you want to save the line coverage to a different path/name you can run

```bash
drcov2lcov --input <input_file> --output <output_file>
```

## Installing

You can either clone this repository and run
```bash
cargo build --release
```
if you want to build this project from source or install directly from `crates.io` by running

```bash
cargo install drcov2lcov
```

