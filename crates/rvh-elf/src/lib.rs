//! A minimal ELF32 reader.
//!
//! Scope is deliberate: statically linked, little-endian, 32-bit RISC-V
//! executables with `PT_LOAD` segments. No sections, no symbols, no relocations,
//! no dynamic linking. That is what `riscv64-linux-gnu-as` plus `ld -m
//! elf32lriscv` produce for a bare-metal program, and it is all a flat-memory
//! simulator needs.

#![forbid(unsafe_code)]

/// One loaded segment: the bytes from the file, and how much memory it claims.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// Virtual address the segment loads at.
    pub addr: u32,
    /// Bytes present in the file.
    pub data: Vec<u8>,
    /// Size in memory; the part beyond `data` is zero-filled (`.bss`).
    pub mem_size: u32,
    /// Segment flags: 1 execute, 2 write, 4 read.
    pub flags: u32,
}

impl Segment {
    #[must_use]
    pub fn is_executable(&self) -> bool {
        self.flags & 1 != 0
    }
}

/// What a loader needs: where to start, and what to put in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Executable {
    pub entry: u32,
    pub segments: Vec<Segment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    TooShort(usize),
    BadMagic,
    NotElf32,
    NotLittleEndian,
    NotRiscV(u16),
    /// `ET_REL` (an object file) or `ET_DYN` (needs relocation).
    UnsupportedType(u16),
    BadProgramHeaders,
    /// A `PT_LOAD` segment reaches past the end of the file.
    TruncatedSegment {
        offset: u32,
        size: u32,
    },
    NoLoadableSegment,
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooShort(len) => write!(
                formatter,
                "file is {len} bytes, too short for an ELF header"
            ),
            Self::BadMagic => write!(formatter, "not an ELF file"),
            Self::NotElf32 => write!(formatter, "not a 32-bit ELF; only ELF32 is supported"),
            Self::NotLittleEndian => write!(formatter, "not little-endian"),
            Self::NotRiscV(machine) => {
                write!(formatter, "not RISC-V (e_machine = {machine})")
            }
            Self::UnsupportedType(kind) => write!(
                formatter,
                "unsupported ELF type {kind}; link the object first (`ld -m elf32lriscv`)"
            ),
            Self::BadProgramHeaders => write!(formatter, "program header table is out of range"),
            Self::TruncatedSegment { offset, size } => write!(
                formatter,
                "a segment at offset {offset} claims {size} bytes, past the end of the file"
            ),
            Self::NoLoadableSegment => write!(formatter, "no PT_LOAD segment"),
        }
    }
}

const HEADER_SIZE: usize = 52;
const PHDR_SIZE: usize = 32;
const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const MACHINE_RISCV: u16 = 243;
const PT_LOAD: u32 = 1;
const ET_EXEC: u16 = 2;

fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    let slice = bytes.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([slice[0], slice[1]]))
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

/// Parse an executable. Never panics; malformed input returns an [`Error`].
pub fn parse(bytes: &[u8]) -> Result<Executable, Error> {
    if bytes.len() < HEADER_SIZE {
        return Err(Error::TooShort(bytes.len()));
    }
    if bytes.get(..4) != Some(&ELF_MAGIC) {
        return Err(Error::BadMagic);
    }
    // e_ident[4] class: 1 = 32-bit. e_ident[5] data: 1 = little-endian.
    match bytes[4] {
        1 => {}
        _ => return Err(Error::NotElf32),
    }
    match bytes[5] {
        1 => {}
        _ => return Err(Error::NotLittleEndian),
    }
    let kind = u16_at(bytes, 16).ok_or(Error::TooShort(bytes.len()))?;
    if kind != ET_EXEC {
        return Err(Error::UnsupportedType(kind));
    }
    let machine = u16_at(bytes, 18).ok_or(Error::TooShort(bytes.len()))?;
    if machine != MACHINE_RISCV {
        return Err(Error::NotRiscV(machine));
    }

    let entry = u32_at(bytes, 24).ok_or(Error::TooShort(bytes.len()))?;
    let phoff = u32_at(bytes, 28).ok_or(Error::TooShort(bytes.len()))? as usize;
    let phentsize = u16_at(bytes, 42).ok_or(Error::TooShort(bytes.len()))? as usize;
    let phnum = u16_at(bytes, 44).ok_or(Error::TooShort(bytes.len()))? as usize;

    if phnum == 0 {
        return Err(Error::NoLoadableSegment);
    }
    let ph_size = phentsize.max(PHDR_SIZE);
    let table_end = phoff
        .checked_add(ph_size.checked_mul(phnum).ok_or(Error::BadProgramHeaders)?)
        .ok_or(Error::BadProgramHeaders)?;
    if table_end > bytes.len() {
        return Err(Error::BadProgramHeaders);
    }

    let mut segments = Vec::new();
    for index in 0..phnum {
        let base = phoff + index * ph_size;
        let kind = u32_at(bytes, base).ok_or(Error::BadProgramHeaders)?;
        if kind != PT_LOAD {
            continue;
        }
        let offset = u32_at(bytes, base + 4).ok_or(Error::BadProgramHeaders)?;
        let addr = u32_at(bytes, base + 8).ok_or(Error::BadProgramHeaders)?;
        let filesz = u32_at(bytes, base + 16).ok_or(Error::BadProgramHeaders)?;
        let memsz = u32_at(bytes, base + 20).ok_or(Error::BadProgramHeaders)?;
        let flags = u32_at(bytes, base + 24).ok_or(Error::BadProgramHeaders)?;

        let start = offset as usize;
        let end = start
            .checked_add(filesz as usize)
            .ok_or(Error::TruncatedSegment {
                offset,
                size: filesz,
            })?;
        if end > bytes.len() {
            return Err(Error::TruncatedSegment {
                offset,
                size: filesz,
            });
        }
        segments.push(Segment {
            addr,
            data: bytes[start..end].to_vec(),
            mem_size: memsz.max(filesz),
            flags,
        });
    }

    if segments.is_empty() {
        return Err(Error::NoLoadableSegment);
    }
    Ok(Executable { entry, segments })
}

/// Write an executable that [`parse`] reads back.
///
/// Layout: header, one program header per segment, then the segment payloads,
/// each aligned to four bytes. No section headers: this file is for a loader, and
/// `readelf -l` reads segments fine without them.
#[must_use]
pub fn write(executable: &Executable) -> Vec<u8> {
    let phnum = executable.segments.len() as u16;
    let phoff = HEADER_SIZE as u32;
    let mut payload = HEADER_SIZE + PHDR_SIZE * executable.segments.len();
    let mut offsets = Vec::with_capacity(executable.segments.len());
    for segment in &executable.segments {
        payload = (payload + 3) & !3;
        offsets.push(payload as u32);
        payload += segment.data.len();
    }

    let mut bytes = vec![0u8; payload];
    bytes[..4].copy_from_slice(&ELF_MAGIC);
    bytes[4] = 1; // ELFCLASS32
    bytes[5] = 1; // little-endian
    bytes[6] = 1; // EV_CURRENT
    bytes[16..18].copy_from_slice(&ET_EXEC.to_le_bytes());
    bytes[18..20].copy_from_slice(&MACHINE_RISCV.to_le_bytes());
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[24..28].copy_from_slice(&executable.entry.to_le_bytes());
    bytes[28..32].copy_from_slice(&phoff.to_le_bytes());
    bytes[40..42].copy_from_slice(&(HEADER_SIZE as u16).to_le_bytes());
    bytes[42..44].copy_from_slice(&(PHDR_SIZE as u16).to_le_bytes());
    bytes[44..46].copy_from_slice(&phnum.to_le_bytes());

    for (index, segment) in executable.segments.iter().enumerate() {
        let base = HEADER_SIZE + index * PHDR_SIZE;
        let offset = offsets[index];
        bytes[base..base + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
        bytes[base + 4..base + 8].copy_from_slice(&offset.to_le_bytes());
        bytes[base + 8..base + 12].copy_from_slice(&segment.addr.to_le_bytes());
        bytes[base + 12..base + 16].copy_from_slice(&segment.addr.to_le_bytes());
        bytes[base + 16..base + 20].copy_from_slice(&(segment.data.len() as u32).to_le_bytes());
        bytes[base + 20..base + 24].copy_from_slice(&segment.mem_size.to_le_bytes());
        bytes[base + 24..base + 28].copy_from_slice(&segment.flags.to_le_bytes());
        bytes[base + 28..base + 32].copy_from_slice(&4u32.to_le_bytes());

        let start = offset as usize;
        bytes[start..start + segment.data.len()].copy_from_slice(&segment.data);
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal ELF32 executable with one `PT_LOAD` segment.
    fn sample(entry: u32, addr: u32, code: &[u8], mem_size: u32) -> Vec<u8> {
        let header_size = HEADER_SIZE;
        let phdr_size = PHDR_SIZE;
        let code_offset = header_size + phdr_size;
        let mut bytes = vec![0u8; code_offset];
        bytes[..4].copy_from_slice(&ELF_MAGIC);
        bytes[4] = 1; // ELFCLASS32
        bytes[5] = 1; // little-endian
        bytes[6] = 1; // version
        bytes[16..18].copy_from_slice(&ET_EXEC.to_le_bytes());
        bytes[18..20].copy_from_slice(&MACHINE_RISCV.to_le_bytes());
        bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
        bytes[24..28].copy_from_slice(&entry.to_le_bytes());
        bytes[28..32].copy_from_slice(&(header_size as u32).to_le_bytes());
        bytes[40..42].copy_from_slice(&(header_size as u16).to_le_bytes());
        bytes[42..44].copy_from_slice(&(phdr_size as u16).to_le_bytes());
        bytes[44..46].copy_from_slice(&1u16.to_le_bytes());

        let phdr = header_size;
        bytes[phdr..phdr + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
        bytes[phdr + 4..phdr + 8].copy_from_slice(&(code_offset as u32).to_le_bytes());
        bytes[phdr + 8..phdr + 12].copy_from_slice(&addr.to_le_bytes());
        bytes[phdr + 12..phdr + 16].copy_from_slice(&addr.to_le_bytes());
        bytes[phdr + 16..phdr + 20].copy_from_slice(&(code.len() as u32).to_le_bytes());
        bytes[phdr + 20..phdr + 24].copy_from_slice(&mem_size.to_le_bytes());
        bytes[phdr + 24..phdr + 28].copy_from_slice(&5u32.to_le_bytes());

        bytes.extend_from_slice(code);
        bytes
    }

    #[test]
    fn write_round_trips_through_parse() {
        let original = Executable {
            entry: 0x100,
            segments: vec![
                Segment {
                    addr: 0x100,
                    data: vec![0x13, 0x05, 0x70, 0x00],
                    mem_size: 4,
                    flags: 5,
                },
                Segment {
                    addr: 0x200,
                    data: vec![1, 2, 3, 4, 5],
                    mem_size: 8,
                    flags: 6,
                },
            ],
        };
        let bytes = write(&original);
        let parsed = parse(&bytes).expect("round trip parses");
        assert_eq!(parsed.entry, original.entry);
        assert_eq!(parsed.segments.len(), 2);
        assert_eq!(parsed.segments[0].data, original.segments[0].data);
        assert_eq!(parsed.segments[1].addr, 0x200);
        assert_eq!(parsed.segments[1].data, original.segments[1].data);
        assert_eq!(parsed.segments[1].mem_size, 8);
    }

    #[test]
    fn reads_entry_and_one_segment() {
        let executable = parse(&sample(0x100, 0x100, &[0x13, 0, 0, 0], 8)).expect("valid");
        assert_eq!(executable.entry, 0x100);
        assert_eq!(executable.segments.len(), 1);
        assert_eq!(executable.segments[0].addr, 0x100);
        assert_eq!(executable.segments[0].data, vec![0x13, 0, 0, 0]);
        assert_eq!(executable.segments[0].mem_size, 8, "bss is accounted for");
        assert!(executable.segments[0].is_executable());
    }

    #[test]
    fn rejects_short_files() {
        assert_eq!(parse(&[0x7f, b'E']), Err(Error::TooShort(2)));
    }

    #[test]
    fn rejects_non_elf() {
        let mut bytes = vec![0u8; HEADER_SIZE];
        bytes[..4].copy_from_slice(b"\x7fABC");
        assert_eq!(parse(&bytes), Err(Error::BadMagic));
    }

    #[test]
    fn rejects_a_64_bit_file() {
        let mut bytes = sample(0, 0, &[], 0);
        bytes[4] = 2;
        assert_eq!(parse(&bytes), Err(Error::NotElf32));
    }

    #[test]
    fn rejects_an_object_file() {
        let mut bytes = sample(0, 0, &[], 0);
        bytes[16..18].copy_from_slice(&1u16.to_le_bytes()); // ET_REL
        assert!(matches!(parse(&bytes), Err(Error::UnsupportedType(1))));
    }

    #[test]
    fn rejects_a_truncated_segment() {
        let mut bytes = sample(0, 0, &[0u8; 4], 4);
        bytes.truncate(bytes.len() - 2);
        assert!(matches!(parse(&bytes), Err(Error::TruncatedSegment { .. })));
    }

    #[test]
    fn rejects_a_missing_load_segment() {
        let mut bytes = sample(0, 0, &[0u8; 4], 4);
        bytes[HEADER_SIZE..HEADER_SIZE + 4].copy_from_slice(&4u32.to_le_bytes()); // PT_NOTE
        assert_eq!(parse(&bytes), Err(Error::NoLoadableSegment));
    }
}
