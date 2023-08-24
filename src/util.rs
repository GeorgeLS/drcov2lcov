use regex::bytes::Captures;

pub struct Hex<T> {
    pub value: T,
}

impl std::str::FromStr for Hex<usize> {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let value = usize::from_str_radix(s, 16)?;
        Ok(Self { value })
    }
}

pub fn parse_capture_group<F: std::str::FromStr>(cap: &Captures<'_>, name: &str) -> Option<F> {
    let res = cap
        .name(name)
        .and_then(|m| String::from_utf8_lossy(m.as_bytes()).parse::<F>().ok());

    res
}
