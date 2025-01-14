use std::{
    fs::File,
    io::{self, Write},
};
//use std::time::Instant;
use ahash::AHashMap;
use memchr::memchr2_iter;
use memmap2::MmapOptions;

pub const FILE: &str = "/home/jan/Dev-Projects/repos/contributions/one_billion_rows/create_measurements/measurements.txt";
pub const NEWLINE: u8 = 10;
pub const SEMICOLON: u8 = 59;
pub const MINUS: u8 = 45;
pub const PERIOD: u8 = 46;
pub const NUM_CPUS: usize = 32; // I only have 18!
pub const NUM_STATIONS: usize = 413;

#[derive(Debug)]
struct Aggregator {
    name: String,
    min: i32,
    max: i32,
    sum: i64,
    count: u64,
}

impl Default for Aggregator {
    fn default() -> Self {
        Self {
            name: String::new(),
            min: i32::MAX,
            max: i32::MIN,
            sum: 0,
            count: 0,
        }
    }
}

fn find_next_newline(start: usize, buffer: &[u8]) -> usize {
    let mut pos = start;
    while pos < buffer.len() {
        if buffer[pos] == NEWLINE {
            return pos + 1;
        }
        pos += 1;
    }
    panic!("Oops - no line found, your algorithm is broken.")
}

fn parse_ascii_digits(buffer: &[u8]) -> i32 {
    let size = buffer.len();
    let mut negative_mul = 1;
    let mut accumulator = 0;
    let mut positional_mul = 10_i32.pow(size as u32 - 2);
    for i in 0..size {
        match buffer[i] {
            MINUS => {
                negative_mul = -1;
                positional_mul /= 10;
            }
            PERIOD => {
                // Do nothing
            }
            48..=57 => {
                // Digits
                let digit = buffer[i] as i32 - 48;
                accumulator += digit * positional_mul;
                positional_mul /= 10;
            }
            _ => panic!("Unhandled ASCII numerical symbol: {}", buffer[i]),
        }
    }
    accumulator *= negative_mul;
    accumulator
}

fn scan_ascii_chunk(start: usize, end: usize, buffer: &[u8]) -> Vec<Aggregator> {
    let mut pos = start;
    let mut line_start = start;
    let mut name_end = start;
    let mut val_start = start;
    let iter = memchr2_iter(SEMICOLON, NEWLINE, &buffer[start..end]);
    let counter = iter.fold(
        AHashMap::with_capacity(NUM_STATIONS),
        |mut acc, found_idx| {
            match buffer[start + found_idx] {
                SEMICOLON => {
                    // From line_start to here-1 is the name
                    name_end = start + found_idx;
                    val_start = start + found_idx + 1;
                }
                NEWLINE => {
                    // This is the end of the line
                    let station = &buffer[line_start..name_end];
                    let value_ascii = &buffer[val_start..start + found_idx];
                    let value = parse_ascii_digits(value_ascii);
                    let entry = acc.entry(station).or_insert(Aggregator::default());
                    if entry.name.is_empty() {
                        entry.name = String::from_utf8_lossy(station).to_string();
                    }
                    entry.max = i32::max(value, entry.max);
                    entry.min = i32::min(value, entry.min);
                    entry.sum += value as i64;
                    entry.count += 1;

                    // Therefore the next line starts at the next character
                    line_start = start + found_idx + 1;
                }
                _ => {}
            }
            acc
        },
    );

    counter.into_iter().map(|(_k, v)| v).collect()
}

pub fn read_file() -> anyhow::Result<()> {
    //let start = Instant::now();
    let file = File::open(FILE)?;
    let mapped_file = unsafe { MmapOptions::new().map(&file)? };
    let size = mapped_file.len();

    // Divide the mapped memory into roughly equal chunks. We'll store
    // a starting point and ending point for each chunk. Starting
    // points are adjusted to seek forward to the next newline.
    let chunk_length = size / NUM_CPUS;
    let mut starting_points: Vec<usize> = (0..NUM_CPUS).map(|n| n * chunk_length).collect();
    for i in 1..NUM_CPUS {
        starting_points[i] = find_next_newline(starting_points[i], &mapped_file);
    }

    let mut ending_points = vec![0usize; NUM_CPUS];
    for i in 0..NUM_CPUS - 1 {
        ending_points[i] = starting_points[i + 1];
    }
    ending_points[NUM_CPUS - 1] = size;

    // Using a scoped pool to make it easy to share the immutable data from above.
    // Scan each segment to find station names and values.
    let mut result = Vec::with_capacity(NUM_STATIONS);
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(NUM_CPUS);
        for thread in 0..NUM_CPUS {
            let start = starting_points[thread];
            let end = ending_points[thread];
            let buffer = &mapped_file;
            let handle = scope.spawn(move || scan_ascii_chunk(start, end, &buffer));
            handles.push(handle);
        }

        // Aggregate the results
        for handle in handles {
            let chunk_result = handle.join().unwrap();
            if result.is_empty() {
                result.extend(chunk_result);
            } else {
                chunk_result.into_iter().for_each(|v| {
                    if let Some(agg) = result.iter_mut().find(|a| a.name == v.name) {
                        agg.sum += v.sum;
                        agg.count += v.count;
                        agg.max = i32::max(agg.max, v.max);
                        agg.min = i32::min(agg.min, v.min);
                    } else {
                        result.push(v);
                    }
                });
            }
        }
    });

    result.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(result.len(), NUM_STATIONS);

    Ok(())
}
