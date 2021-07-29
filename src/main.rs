use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs::{self, create_dir_all, DirEntry, File};
use std::hash::Hasher;
use std::io::{self, Read, Write};
use std::path::{Component, Path};

use base64::decode;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;

fn visit_dirs(dir: &Path, cb: &mut dyn FnMut(&DirEntry)) -> io::Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                visit_dirs(&path, cb)?;
            } else {
                cb(&entry);
            }
        }
    }
    Ok(())
}

fn file_sep(path: &Path, hash: &str) -> String {
    format!("===={}|{}====\n", path.to_string_lossy(), hash)
}

const ENCODE_OUTPUT: &str = "out.out";
const DECODE_OUTPUT: &str = "output";
const IGNORED_FILE_DIR: [&str; 6] = [
    ".git",
    "Cargo.lock",
    "target",
    "node_modules",
    ENCODE_OUTPUT,
    DECODE_OUTPUT,
];

enum Mode {
    Plain,
    Base64,
    CompressedTxt,
    CompressedBinary,
}

fn main() -> io::Result<()> {
    let mut args = env::args();

    args.next();
    let command = args.next();
    let command = command.as_deref();

    let mode = args.next();
    let mode = mode.as_deref().unwrap_or("--plain");

    let mode = match mode {
        "--plain" => Mode::Plain,
        "--base64" => Mode::Base64,
        "--binary" => Mode::CompressedBinary,
        "--text" => Mode::CompressedTxt,
        _ => {
            panic!(
                "not support {}, available option are --[plain|base64|binary|text]",
                mode
            );
        }
    };

    if let Some("encode") = command {
        encode_dir(".".as_ref(), mode)?;
    } else if let Some("decode") = command {
        decode_dir(".".as_ref(), mode)?;
    } else {
        eprintln!("command is `decode` or `encode`")
    }

    Ok(())
}

fn create_file_sep(path: &Path, buffer: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    hasher.write(buffer);
    file_sep(path, &hasher.finish().to_string())
}

fn encode_dir(path: &Path, mode: Mode) -> io::Result<()> {
    let mut out_file = File::create(ENCODE_OUTPUT)?;

    let mut e = ZlibEncoder::new(Vec::new(), Compression::default());

    visit_dirs(path, &mut |entry| {
        let path = entry.path();
        let ignored = path.components().any(|component| {
            if let Component::Normal(normal) = component {
                // return normal.to_string_lossy() == ".git"
                return IGNORED_FILE_DIR
                    .iter()
                    .any(|p| *p == normal.to_string_lossy());
            }
            false
        });
        if !ignored {
            let mut file = File::open(entry.path()).unwrap();
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer).unwrap();

            let file_sep = create_file_sep(&entry.path(), &buffer);

            match mode {
                Mode::Plain => {
                    out_file.write_all(file_sep.as_bytes()).unwrap();
                    out_file.write_all(&buffer).unwrap();
                    out_file.write_all(b"\n").unwrap();
                }
                Mode::Base64 => {
                    out_file.write_all(file_sep.as_bytes()).unwrap();
                    let base64_str = base64::encode(buffer);
                    out_file.write_all(base64_str.as_bytes()).unwrap();
                    out_file.write_all(b"\n").unwrap();
                }
                Mode::CompressedBinary | Mode::CompressedTxt => {
                    e.write_all(file_sep.as_bytes()).unwrap();
                    let base64_str = base64::encode(&buffer);
                    e.write_all(base64_str.as_bytes()).unwrap();
                    e.write_all(b"\n").unwrap();
                }
            }
        }
    })?;

    match mode {
        Mode::CompressedBinary => {
            let compressed = e.finish().unwrap();
            out_file.write_all(&compressed).unwrap();
        }
        Mode::CompressedTxt => {
            let compressed = e.finish().unwrap();
            out_file
                .write_all(base64::encode(&compressed).as_bytes())
                .unwrap();
        }
        _ => {}
    }
    Ok(())
}

fn decode_dir(path: &Path, mode: Mode) -> io::Result<()> {
    let mut file = File::open(path.to_owned().join(ENCODE_OUTPUT))?;

    let buffer = match mode {
        Mode::CompressedBinary => {
            let mut z = ZlibDecoder::new(file);
            let mut s = String::new();
            z.read_to_string(&mut s)?;
            s
        }
        Mode::CompressedTxt => {
            let mut buffer = String::new();
            file.read_to_string(&mut buffer)?;
            let buffer = decode(buffer).unwrap();
            let mut z = ZlibDecoder::new(&buffer[..]);
            let mut s = String::new();
            z.read_to_string(&mut s)?;
            s
        }
        _ => {
            let mut buffer = String::new();
            file.read_to_string(&mut buffer)?;
            buffer
        }
    };
    let mut buffer: &str = &buffer;

    let mut output_file = None;
    loop {
        if buffer.is_empty() {
            break;
        }
        let split = buffer
            .find('\n')
            .map(|i| i + 1)
            .unwrap_or_else(|| buffer.len());
        let (line, rest) = buffer.split_at(split);
        buffer = rest;

        if line.starts_with("====") && line.ends_with("====\n") {
            let path_hash = &line[4..line.len() - 5];
            let path = path_hash.split('|').next().unwrap();
            let target = Path::new(&format!("./{}", DECODE_OUTPUT)).join(path);
            create_dir_all(target.parent().unwrap())?;
            output_file = Some(File::create(target)?);
        } else if let Some(output) = output_file.as_mut() {
            match mode {
                Mode::Plain => {
                    output.write_all(line.as_bytes())?;
                }
                _ => {
                    let decoded = base64::decode(&line[..line.len() - 1]).unwrap();
                    output.write_all(&decoded)?;
                }
            }
        }
    }

    Ok(())
}
