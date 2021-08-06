// Copyright 2019 CoreOS, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use anyhow::{anyhow, bail, Context, Result};
use bytes::Buf;
use cpio::{write_cpio, NewcBuilder, NewcReader};
use nix::unistd::isatty;
use openat_ext::FileExt;
use serde::Serialize;
use std::convert::TryInto;
use std::fs::{read, write, File, OpenOptions};
use std::io::{copy, stdin, stdout, BufReader, BufWriter, Cursor, Read, Seek, SeekFrom, Write};
use std::iter::repeat;
use std::os::unix::io::AsRawFd;
use std::path::Path;

use crate::cmdline::*;
use crate::install::*;
use crate::io::*;

const FILENAME: &str = "config.ign";
const COREOS_IGNITION_HEADER_MAGIC: &[u8] = b"coreiso+";
const COREOS_IGNITION_HEADER_SIZE: u64 = 24;
const COREOS_KARG_EMBED_AREA_HEADER_MAGIC: &[u8] = b"coreKarg";
const COREOS_KARG_EMBED_AREA_HEADER_SIZE: u64 = 72;
const COREOS_KARG_EMBED_AREA_HEADER_MAX_OFFSETS: usize = 6;
const COREOS_KARG_EMBED_AREA_MAX_SIZE: usize = 2048;

pub fn iso_embed(config: &IsoEmbedConfig) -> Result<()> {
    eprintln!("`iso embed` is deprecated; use `iso ignition embed`.  Continuing.");
    iso_ignition_embed(&IsoIgnitionEmbedConfig {
        force: config.force,
        ignition_file: config.config.clone(),
        output: config.output.clone(),
        input: config.input.clone(),
    })
}

pub fn iso_show(config: &IsoShowConfig) -> Result<()> {
    eprintln!("`iso show` is deprecated; use `iso ignition show`.  Continuing.");
    iso_ignition_show(&IsoIgnitionShowConfig {
        input: config.input.clone(),
        header: false,
    })
}

pub fn iso_remove(config: &IsoRemoveConfig) -> Result<()> {
    eprintln!("`iso remove` is deprecated; use `iso ignition remove`.  Continuing.");
    iso_ignition_remove(&IsoIgnitionRemoveConfig {
        output: config.output.clone(),
        input: config.input.clone(),
    })
}

pub fn iso_ignition_embed(config: &IsoIgnitionEmbedConfig) -> Result<()> {
    let ignition = match config.ignition_file {
        Some(ref ignition_path) => {
            read(ignition_path).with_context(|| format!("reading {}", ignition_path))?
        }
        None => {
            let mut data = Vec::new();
            stdin().read_to_end(&mut data).context("reading stdin")?;
            data
        }
    };

    let mut iso_file = open_live_iso(&config.input, Some(config.output.as_ref()))?;
    let mut iso = IsoConfig::for_file(&mut iso_file)?;

    if !config.force && iso.have_ignition() {
        bail!("This ISO image already has an embedded Ignition config; use -f to force.");
    }

    let cpio = make_cpio(&ignition)?;
    iso.set_ignition(&cpio)?;

    if write_live_iso(&iso, &mut iso_file, config.output.as_ref())? {
        iso.stream_ignition(&mut iso_file, &mut stdout())?;
    }

    Ok(())
}

pub fn iso_ignition_show(config: &IsoIgnitionShowConfig) -> Result<()> {
    let mut iso_file = open_live_iso(&config.input, None)?;
    let iso = IsoConfig::for_file(&mut iso_file)?;
    if config.header {
        serde_json::to_writer_pretty(std::io::stdout(), &iso.ignition)
            .context("failed to serialize header")?;
        std::io::stdout()
            .write_all("\n".as_bytes())
            .context("failed to write newline")?;
    } else {
        if !iso.have_ignition() {
            bail!("No embedded Ignition config.");
        }
        stdout()
            .write_all(&extract_cpio(iso.ignition())?)
            .context("writing output")?;
        stdout().flush().context("flushing output")?;
    }
    Ok(())
}

pub fn iso_ignition_remove(config: &IsoIgnitionRemoveConfig) -> Result<()> {
    let mut iso_file = open_live_iso(&config.input, Some(config.output.as_ref()))?;
    let mut iso = IsoConfig::for_file(&mut iso_file)?;

    iso.set_ignition(&[])?;

    if write_live_iso(&iso, &mut iso_file, config.output.as_ref())? {
        iso.stream_ignition(&mut iso_file, &mut stdout())?;
    }
    Ok(())
}

pub fn pxe_ignition_wrap(config: &PxeIgnitionWrapConfig) -> Result<()> {
    if config.output.is_none()
        && isatty(stdout().as_raw_fd()).context("checking if stdout is a TTY")?
    {
        bail!("Refusing to write binary data to terminal");
    }

    let ignition = match config.ignition_file {
        Some(ref ignition_path) => {
            read(ignition_path).with_context(|| format!("reading {}", ignition_path))?
        }
        None => {
            let mut data = Vec::new();
            stdin().read_to_end(&mut data).context("reading stdin")?;
            data
        }
    };

    let cpio = make_cpio(&ignition)?;

    match &config.output {
        Some(output_path) => {
            write(output_path, cpio).with_context(|| format!("writing {}", output_path))?
        }
        None => {
            stdout().write_all(&cpio).context("writing output")?;
            stdout().flush().context("flushing output")?;
        }
    }
    Ok(())
}

pub fn pxe_ignition_unwrap(config: &PxeIgnitionUnwrapConfig) -> Result<()> {
    let buf = read(&config.input).with_context(|| format!("reading {}", config.input))?;
    stdout()
        .write_all(&extract_cpio(&buf)?)
        .context("writing output")?;
    stdout().flush().context("flushing output")?;
    Ok(())
}

pub fn iso_kargs_modify(config: &IsoKargsModifyConfig) -> Result<()> {
    let mut iso_file = open_live_iso(&config.input, Some(config.output.as_ref()))?;
    let mut iso = IsoConfig::for_file(&mut iso_file)?;

    let kargs = modify_kargs(
        iso.kargs()?,
        &config.append,
        &[],
        &config.replace,
        &config.delete,
    )?;
    iso.set_kargs(&kargs)?;

    if write_live_iso(&iso, &mut iso_file, config.output.as_ref())? {
        iso.stream_kargs(&iso_file, &mut stdout())?;
    }
    Ok(())
}

pub fn iso_kargs_reset(config: &IsoKargsResetConfig) -> Result<()> {
    let mut iso_file = open_live_iso(&config.input, Some(config.output.as_ref()))?;
    let mut iso = IsoConfig::for_file(&mut iso_file)?;

    iso.set_kargs(&iso.kargs_default()?.to_string())?;
    if write_live_iso(&iso, &mut iso_file, config.output.as_ref())? {
        iso.stream_kargs(&iso_file, &mut stdout())?;
    }
    Ok(())
}

pub fn iso_kargs_show(config: &IsoKargsShowConfig) -> Result<()> {
    let mut iso_file = open_live_iso(&config.input, None)?;
    let iso = IsoConfig::for_file(&mut iso_file)?;
    if config.header {
        // XXX
        let karg_areas = KargEmbedAreas::for_file(iso_file.try_clone()?)?;
        serde_json::to_writer_pretty(std::io::stdout(), &karg_areas)
            .context("failed to serialize header")?;
        std::io::stdout()
            .write_all("\n".as_bytes())
            .context("failed to write newline")?;
    } else {
        let kargs = if config.default {
            iso.kargs_default()?
        } else {
            iso.kargs()?
        };
        println!("{}", kargs);
    }
    Ok(())
}

// output_path should be None if not outputting, or Some(output_path_argument)
fn open_live_iso(input_path: &str, output_path: Option<Option<&String>>) -> Result<File> {
    // if output_path is Some(None), we're modifying in place, so we need to
    // open for writing
    OpenOptions::new()
        .read(true)
        .write(matches!(output_path, Some(None)))
        .open(&input_path)
        .with_context(|| format!("opening {}", &input_path))
}

// Returns true if we need to stream to stdout
fn write_live_iso(iso: &IsoConfig, input: &mut File, output_path: Option<&String>) -> Result<bool> {
    match output_path.map(|v| v.as_str()) {
        None => {
            // open_live_iso() opened input for writing
            iso.write(input)?;
            Ok(false)
        }
        Some("-") => {
            if isatty(stdout().as_raw_fd()).context("checking if stdout is a TTY")? {
                bail!("Refusing to write binary data to terminal");
            }
            Ok(true)
        }
        Some(output_path) => {
            let output_dir = Path::new(output_path)
                .parent()
                .with_context(|| format!("no parent directory of {}", output_path))?;
            let mut output = tempfile::Builder::new()
                .prefix(".coreos-installer-temp-")
                .tempfile_in(output_dir)
                .context("creating temporary file")?;
            input.seek(SeekFrom::Start(0)).context("seeking input")?;
            input
                .copy_to(output.as_file_mut())
                .context("copying input to temporary file")?;
            iso.write(output.as_file_mut())?;
            output
                .persist_noclobber(&output_path)
                .map_err(|e| e.error)
                .with_context(|| format!("persisting output file to {}", output_path))?;
            Ok(false)
        }
    }
}

struct IsoConfig {
    ignition: Region,
    kargs_current: Option<String>,
    kargs_default: Option<String>,
}

impl IsoConfig {
    pub fn for_file(file: &mut File) -> Result<Self> {
        let (kargs_current, kargs_default) = if KargEmbedAreas::exists_in(file)? {
            let mut karg_areas = KargEmbedAreas::for_file(file.try_clone()?)?;
            let current = karg_areas.get_current_kargs()?;
            let default = karg_areas.get_default_kargs()?;
            (Some(current), Some(default))
        } else {
            (None, None)
        };

        Ok(Self {
            ignition: ignition_embed_area(file)?,
            kargs_current,
            kargs_default,
        })
    }

    pub fn have_ignition(&self) -> bool {
        self.ignition().iter().any(|v| *v != 0)
    }

    pub fn ignition(&self) -> &[u8] {
        &self.ignition.contents[..]
    }

    pub fn set_ignition(&mut self, data: &[u8]) -> Result<()> {
        let capacity = self.ignition.length;
        if data.len() > capacity {
            bail!(
                "Compressed Ignition config is too large: {} > {}",
                data.len(),
                capacity
            )
        }
        self.ignition.contents.clear();
        self.ignition.contents.extend_from_slice(data);
        self.ignition
            .contents
            .extend(repeat(0).take(capacity - data.len()));
        Ok(())
    }

    pub fn kargs(&self) -> Result<&str> {
        Self::unwrap_kargs(&self.kargs_current)
    }

    pub fn kargs_default(&self) -> Result<&str> {
        Self::unwrap_kargs(&self.kargs_default)
    }

    pub fn set_kargs(&mut self, kargs: &str) -> Result<()> {
        Self::unwrap_kargs(&self.kargs_default)?;
        self.kargs_current = Some(kargs.to_string());
        Ok(())
    }

    fn unwrap_kargs(kargs: &Option<String>) -> Result<&str> {
        kargs
            .as_ref()
            .map(|s| s.as_str())
            .ok_or_else(|| anyhow!("No karg embed areas found; old or corrupted CoreOS ISO image."))
    }

    pub fn write(&self, file: &mut File) -> Result<()> {
        self.ignition.write(file)?;
        if let Some(kargs_current) = &self.kargs_current {
            let mut karg_areas = KargEmbedAreas::for_file(file.try_clone()?)?;
            karg_areas.write_kargs(&kargs_current)?;
        }
        Ok(())
    }

    // XXX temporary API
    pub fn stream_ignition(
        &self,
        input: &mut File,
        writer: &mut (impl Write + ?Sized),
    ) -> Result<()> {
        vec![&self.ignition].stream(input, writer)
    }

    // XXX temporary API
    pub fn stream_kargs(&self, input: &File, writer: &mut (impl Write + ?Sized)) -> Result<()> {
        if let Some(kargs_current) = &self.kargs_current {
            let mut karg_areas = KargEmbedAreas::for_file(input.try_clone()?)?;
            karg_areas.stream(&kargs_current, writer)?;
            Ok(())
        } else {
            bail!("no karg embed areas available");
        }
    }
}

#[derive(Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct Region {
    // sort order is derived from field order
    pub offset: u64,
    pub length: usize,
    #[serde(skip_serializing)]
    pub contents: Vec<u8>,
}

impl Region {
    pub fn read(file: &mut File, offset: u64, length: usize) -> Result<Self> {
        let mut contents = vec![0; length];
        file.seek(SeekFrom::Start(offset))
            .with_context(|| format!("seeking to offset {}", offset))?;
        file.read_exact(&mut contents)
            .with_context(|| format!("reading {} bytes at {}", length, offset))?;
        Ok(Self {
            offset,
            length,
            contents,
        })
    }

    pub fn write(&self, file: &mut File) -> Result<()> {
        self.validate()?;
        file.seek(SeekFrom::Start(self.offset))
            .with_context(|| format!("seeking to offset {}", self.offset))?;
        file.write_all(&self.contents)
            .with_context(|| format!("writing {} bytes at {}", self.length, self.offset))
    }

    pub fn validate(&self) -> Result<()> {
        if self.length != self.contents.len() {
            bail!(
                "expected region contents length {}, found {}",
                self.length,
                self.contents.len()
            );
        }
        Ok(())
    }
}

trait Stream {
    fn stream(&self, input: &mut File, writer: &mut (impl Write + ?Sized)) -> Result<()>;
}

impl Stream for [&Region] {
    fn stream(&self, input: &mut File, writer: &mut (impl Write + ?Sized)) -> Result<()> {
        input.seek(SeekFrom::Start(0)).context("seeking to start")?;

        let mut regions: Vec<&&Region> = self.iter().collect();
        regions.sort_unstable();

        let mut buf = [0u8; BUFFER_SIZE];
        let mut cursor: u64 = 0;

        // validate regions
        for region in &regions {
            region.validate()?;
            if region.offset < cursor {
                bail!(
                    "region starting at {} precedes current offset {}",
                    region.offset,
                    cursor
                );
            }
            cursor = region.offset + region.length as u64;
        }

        // write regions
        cursor = 0;
        for region in &regions {
            assert!(region.offset >= cursor);
            copy_exactly_n(input, writer, region.offset - cursor, &mut buf)
                .with_context(|| format!("copying bytes from {} to {}", cursor, region.offset))?;
            writer.write_all(&region.contents).with_context(|| {
                format!(
                    "writing region for {} at offset {}",
                    region.length, region.offset
                )
            })?;
            cursor = input
                .seek(SeekFrom::Current(region.length as i64))
                .with_context(|| format!("seeking region length {}", region.length))?;
        }

        // write the remainder
        let mut write_buf = BufWriter::with_capacity(BUFFER_SIZE, writer);
        copy(
            &mut BufReader::with_capacity(BUFFER_SIZE, input),
            &mut write_buf,
        )
        .context("copying file")?;
        write_buf.flush().context("flushing output")?;
        Ok(())
    }
}

#[derive(Serialize)]
struct KargEmbedAreas {
    #[serde(skip_serializing)]
    file: File,
    length: usize,
    default_kargs_offset: u64,
    kargs_offsets: Vec<u64>,
}

impl KargEmbedAreas {
    pub fn exists_in(file: &mut File) -> Result<bool> {
        let region = Region::read(
            file,
            32768 - COREOS_IGNITION_HEADER_SIZE - COREOS_KARG_EMBED_AREA_HEADER_SIZE,
            8,
        )
        .context("reading karg embed magic number")?;
        Ok(region.contents == COREOS_KARG_EMBED_AREA_HEADER_MAGIC)
    }

    fn for_file(mut file: File) -> Result<Self> {
        // The ISO 9660 System Area is 32 KiB. Karg embed area information is located in the 72 bytes
        // before the initrd embed area (see EmbedArea below):
        // 8 bytes: magic string "coreKarg"
        // 8 bytes little-endian: length of karg embed areas
        // 8 bytes little-endian: offset to default kargs
        // 8 bytes little-endian x 6: offsets to karg embed areas
        let region = Region::read(
            &mut file,
            32768 - COREOS_IGNITION_HEADER_SIZE - COREOS_KARG_EMBED_AREA_HEADER_SIZE,
            COREOS_KARG_EMBED_AREA_HEADER_SIZE as usize,
        )
        .context("reading karg embed header")?;
        let mut header = &region.contents[..];
        // magic number
        if header.copy_to_bytes(8) != COREOS_KARG_EMBED_AREA_HEADER_MAGIC {
            bail!("No karg embed areas found; old or corrupted CoreOS ISO image.");
        }
        // length
        let length: usize = header
            .get_u64_le()
            .try_into()
            .context("karg embed area length too large to allocate")?;
        // sanity-check against a reasonable limit
        if length > COREOS_KARG_EMBED_AREA_MAX_SIZE {
            bail!(
                "karg embed area length larger than {} (found {})",
                COREOS_KARG_EMBED_AREA_MAX_SIZE,
                length
            );
        }

        let metadata = file.metadata().context("reading metadata for ISO")?;
        let iso_size = metadata.len();

        // default kargs
        let default_kargs_offset = header.get_u64_le();
        if default_kargs_offset + (length as u64) > iso_size {
            bail!(
                "Default kargs area end outside ISO ({}+{} vs {})",
                default_kargs_offset,
                length,
                iso_size
            );
        }

        // offsets
        let mut kargs_offsets: Vec<u64> = Vec::new();
        while kargs_offsets.len() < COREOS_KARG_EMBED_AREA_HEADER_MAX_OFFSETS {
            let offset = header.get_u64_le();
            if offset == 0 {
                break;
            } else if offset + (length as u64) > iso_size {
                bail!(
                    "Kargs area end outside ISO ({}+{} vs {})",
                    offset,
                    length,
                    iso_size
                );
            }
            kargs_offsets.push(offset);
        }

        // we need ordered offsets when streaming
        kargs_offsets.sort_unstable();

        // we expect at least one
        if kargs_offsets.is_empty() {
            bail!("No karg embed areas found; corrupted CoreOS ISO image.");
        }

        Ok(KargEmbedAreas {
            file,
            length,
            default_kargs_offset,
            kargs_offsets,
        })
    }

    fn get_current_kargs(&mut self) -> Result<String> {
        // really, we could just get the kargs from the first file, but let's sanity-check that all
        // the offsets have the same kargs
        let mut first_kargs: Option<String> = None;
        for offset in &self.kargs_offsets {
            let kargs = get_kargs_at_offset(&mut self.file, self.length, *offset)?;
            if let Some(ref first_kargs) = first_kargs {
                if &kargs != first_kargs {
                    bail!(
                        "kargs don't match at all offsets! (expected '{}', but offset {} has: '{}')",
                        first_kargs,
                        *offset,
                        kargs
                    );
                }
            } else {
                first_kargs = Some(kargs);
            }
        }
        Ok(first_kargs.expect("at least one area offset"))
    }

    fn get_default_kargs(&mut self) -> Result<String> {
        get_kargs_at_offset(&mut self.file, self.length, self.default_kargs_offset)
    }

    fn format_embed_area(&mut self, kargs: &str) -> Result<Vec<u8>> {
        let kargs: String = kargs.trim().to_string() + "\n";
        if kargs.len() > self.length {
            bail!(
                "kargs too large for area: {} vs {}",
                kargs.len(),
                self.length
            );
        }
        let mut new_area = vec![b'#'; self.length];
        new_area[..kargs.len()].copy_from_slice(kargs.as_bytes());
        Ok(new_area)
    }

    fn stream(&mut self, kargs: &str, writer: &mut (impl Write + ?Sized)) -> Result<()> {
        let mut buf = [0u8; BUFFER_SIZE];
        let new_area = self.format_embed_area(kargs)?;

        self.file
            .seek(SeekFrom::Start(0))
            .context("seeking to start")?;
        let mut cursor: u64 = 0;

        for offset in &self.kargs_offsets {
            copy_exactly_n(&mut self.file, writer, *offset - cursor, &mut buf)
                .with_context(|| format!("copying bytes from {} to {}", cursor, *offset))?;
            writer
                .write_all(&new_area)
                .with_context(|| format!("writing karg embed area at offset {}", *offset))?;
            cursor = self
                .file
                .seek(SeekFrom::Current(self.length as i64))
                .with_context(|| format!("seeking length of karg embed area {}", self.length))?;
        }

        // write the remainder
        let mut write_buf = BufWriter::with_capacity(BUFFER_SIZE, writer);
        copy(
            &mut BufReader::with_capacity(BUFFER_SIZE, &mut self.file),
            &mut write_buf,
        )
        .context("copying file")?;
        write_buf.flush().context("flushing output")?;
        Ok(())
    }

    fn write_kargs(&mut self, kargs: &str) -> Result<()> {
        let new_area = self.format_embed_area(kargs)?;

        for offset in &self.kargs_offsets {
            self.file
                .seek(SeekFrom::Start(*offset))
                .with_context(|| format!("seeking to karg area offset {}", *offset))?;
            self.file
                .write_all(&new_area)
                .with_context(|| format!("writing karg embed area at offset {}", *offset))?;
        }
        Ok(())
    }
}

// This is purposely not an impl function because we need to be able to call it while borrowing
// parts of the struct (e.g. when iterating over the offsets).
fn get_kargs_at_offset(file: &mut File, area_length: usize, offset: u64) -> Result<String> {
    file.seek(SeekFrom::Start(offset))
        .with_context(|| format!("seeking to karg area offset {}", offset))?;
    let area = {
        let mut buf = vec![0u8; area_length];
        file.read_exact(&mut buf)
            .with_context(|| format!("reading karg area at offset {}", offset))?;
        String::from_utf8(buf).context("invalid UTF-8 in karg area")?
    };
    Ok(area.trim_end_matches('#').trim().into())
}

fn ignition_embed_area(file: &mut File) -> Result<Region> {
    // The ISO 9660 System Area is 32 KiB.  The last 24 bytes should be:
    // 8 bytes: magic string "coreiso+"
    // 8 bytes little-endian: offset of embed area from start of file
    // 8 bytes little-endian: length of embed area
    let region = Region::read(
        file,
        32768 - COREOS_IGNITION_HEADER_SIZE,
        COREOS_IGNITION_HEADER_SIZE as usize,
    )
    .context("reading Ignition embed header")?;
    let mut header = &region.contents[..];
    // magic number
    if header.copy_to_bytes(8) != COREOS_IGNITION_HEADER_MAGIC {
        bail!("Unrecognized CoreOS ISO image.");
    }
    // offset
    let offset = header.get_u64_le();
    // length
    let length: usize = header
        .get_u64_le()
        .try_into()
        .context("embed area too large to allocate")?;
    // read (checks offset/length as a side effect)
    Region::read(file, offset, length).context("reading Ignition embed area")
}

/// Make a gzipped CPIO archive containing the specified Ignition config.
fn make_cpio(ignition: &[u8]) -> Result<Vec<u8>> {
    use xz2::stream::{Check, Stream};
    use xz2::write::XzEncoder;

    let mut result = Cursor::new(Vec::new());
    // kernel requires CRC32: https://www.kernel.org/doc/Documentation/xz.txt
    let encoder = XzEncoder::new_stream(
        &mut result,
        Stream::new_easy_encoder(9, Check::Crc32).context("creating XZ encoder")?,
    );
    let mut input_files = vec![(
        // S_IFREG | 0644
        NewcBuilder::new(FILENAME).mode(0o100_644),
        Cursor::new(ignition),
    )];
    write_cpio(input_files.drain(..), encoder).context("writing CPIO archive")?;
    Ok(result.into_inner())
}

/// Extract a gzipped CPIO archive and return the contents of the Ignition
/// config.
fn extract_cpio(buf: &[u8]) -> Result<Vec<u8>> {
    // older versions of this program, and its predecessor, compressed
    // with gzip
    let mut decompressor = DecompressReader::new(BufReader::new(buf))?;
    loop {
        let mut reader = NewcReader::new(decompressor).context("reading CPIO entry")?;
        let entry = reader.entry();
        if entry.is_trailer() {
            bail!("couldn't find Ignition config in archive");
        }
        if entry.name() == FILENAME {
            let mut result = Vec::with_capacity(entry.file_size() as usize);
            reader
                .read_to_end(&mut result)
                .context("reading CPIO entry contents")?;
            return Ok(result);
        }
        decompressor = reader.finish().context("finishing reading CPIO entry")?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpio_roundtrip() {
        let input = r#"{}"#;
        let cpio = make_cpio(input.as_bytes()).unwrap();
        let output = extract_cpio(&cpio).unwrap();
        assert_eq!(input.as_bytes(), output.as_slice());
    }
}
