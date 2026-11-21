//! Target strings: base ISA plus extension set.

use std::fmt;

use crate::extensions;

/// Why a target string was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetError {
    Empty,
    UnknownBase(String),
    /// A base the database can name but the machine does not implement.
    UnsupportedBase(&'static str),
    UnknownExtension(String),
}

impl fmt::Display for TargetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "empty architecture string"),
            Self::UnsupportedBase(base) => write!(
                f,
                "`{base}` is not implemented; only rv32i is available today (ADR-0012)"
            ),
            Self::UnknownBase(base) => write!(
                f,
                "unknown base `{base}`; expected rv32i, rv32e, rv64i or rv64e"
            ),
            Self::UnknownExtension(ext) => write!(
                f,
                "unknown extension `{ext}`; the database has {}",
                extensions().join(", ")
            ),
        }
    }
}

impl std::error::Error for TargetError {}

const BASES: &[&str] = &["rv32i", "rv32e", "rv64i", "rv64e"];

/// A one-character extension as a static string, for the extension lookup.
fn letters_of(character: char) -> String {
    character.to_string()
}

/// A resolved target: one base ISA and the extensions that apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    base: &'static str,
    extensions: Vec<&'static str>,
}

impl Default for Target {
    fn default() -> Self {
        Self::rv32i()
    }
}

impl Target {
    #[must_use]
    pub fn rv32i() -> Self {
        Self {
            base: "rv32i",
            extensions: vec!["i"],
        }
    }

    /// Parse a canonical architecture string such as `rv32i_zicsr`.
    ///
    /// Extension closure (`M` implying `Zmmul`, and so on) is not implemented
    /// yet; see ADR-0003. Until it is, an implied extension must be written out.
    pub fn parse(arch: &str) -> Result<Self, TargetError> {
        let arch = arch.trim();
        if arch.is_empty() {
            return Err(TargetError::Empty);
        }
        // The base is a prefix of the string: `rv32im` is `rv32i` with the
        // single-letter extension `m`, written without a separator (canonical
        // RISC-V), while `rv32i_m` is the same target spelled differently.
        let Some(&base) = BASES
            .iter()
            .filter(|known| arch.starts_with(**known))
            .max_by_key(|known| known.len())
        else {
            return Err(TargetError::UnknownBase(arch.to_string()));
        };
        let attached = &arch[base.len()..];
        let mut parts = attached.split('_');
        let letters = parts.next().unwrap_or_default();
        let letters: Vec<&'static str> = {
            let mut resolved = Vec::new();
            for character in letters.chars() {
                let name = &letters_of(character);
                let Some(&known) = extensions().iter().find(|ext| **ext == *name) else {
                    return Err(TargetError::UnknownExtension(character.to_string()));
                };
                resolved.push(known);
            }
            resolved
        };
        // Fail fast on 64-bit: it needs a wider datapath, and running 32-bit code
        // under that name would be silently wrong (ADR-0012). `rv32e` is
        // supported: E is the same instruction set with 16 registers, so it
        // shares the `i` table and adds a register limit (ADR-0015).
        if base.starts_with("rv64") {
            return Err(TargetError::UnsupportedBase(base));
        }

        // E draws its instructions from the `i` table; `registers()` carries the
        // difference.
        let mut resolved: Vec<&'static str> = vec!["i"];
        resolved.extend(letters);

        for ext in parts {
            if ext.is_empty() {
                continue;
            }
            let Some(&known) = extensions().iter().find(|name| **name == ext) else {
                return Err(TargetError::UnknownExtension(ext.to_string()));
            };
            resolved.push(known);
        }
        Ok(Self {
            base,
            extensions: resolved,
        })
    }

    #[must_use]
    pub fn base(&self) -> &'static str {
        self.base
    }

    #[must_use]
    pub fn extensions(&self) -> &[&'static str] {
        &self.extensions
    }

    #[must_use]
    pub fn has(&self, extension: &str) -> bool {
        self.extensions.contains(&extension)
    }

    /// True when the instruction belongs to this target.
    #[must_use]
    pub fn allows(&self, insn: &crate::Insn) -> bool {
        self.has(insn.ext)
    }

    /// How many registers the base provides: 16 for RV32E, 32 otherwise.
    #[must_use]
    pub fn registers(&self) -> u8 {
        if self.base == "rv32e" {
            16
        } else {
            32
        }
    }

    /// True when the target is the embedded base.
    #[must_use]
    pub fn is_embedded(&self) -> bool {
        self.base == "rv32e"
    }

    /// Canonical architecture string, e.g. `rv32imac_zicsr`.
    ///
    /// Single-letter extensions are concatenated in the order the specification
    /// gives (`IMAFDQLCBKJTPVH`); multi-letter ones follow, separated by `_`.
    #[must_use]
    pub fn name(&self) -> String {
        const ORDER: &str = "imafdqlcbkjptvh";
        let mut singles: Vec<&'static str> = Vec::new();
        let mut multi: Vec<&'static str> = Vec::new();
        for extension in self.extensions.iter().skip(1) {
            if extension.len() == 1 {
                singles.push(extension);
            } else {
                multi.push(extension);
            }
        }
        singles.sort_by_key(|extension| ORDER.find(extension).unwrap_or(usize::MAX));
        let mut out = String::from(self.base);
        out.extend(singles);
        for extension in multi {
            out.push('_');
            out.push_str(extension);
        }
        out
    }
}

impl fmt::Display for Target {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lookup;

    #[test]
    fn default_is_rv32i() {
        let target = Target::default();
        assert_eq!(target.base(), "rv32i");
        assert!(target.has("i"));
        assert!(!target.has("zicsr"));
    }

    #[test]
    fn parses_two_extension_set() {
        let target = Target::parse("rv32i_zicsr").expect("valid target");
        assert_eq!(target.name(), "rv32i_zicsr");
        assert!(target.has("zicsr"));
        assert!(target.allows(lookup("csrrw").expect("csrrw")));
        assert!(!Target::default().allows(lookup("csrrw").expect("csrrw")));
    }

    #[test]
    fn rejects_unknown_extensions() {
        let error = Target::parse("rv32i_q").expect_err("Q is not in the database yet");
        assert!(matches!(error, TargetError::UnknownExtension(_)));
    }

    #[test]
    fn parses_the_m_extension() {
        let target = Target::parse("rv32i_m").expect("M is in the database");
        assert!(target.has("m"));
        assert!(target.allows(lookup("mul").expect("mul")));
        assert_eq!(target.name(), "rv32im");
    }

    #[test]
    fn rejects_bases_that_are_not_implemented() {
        for base in ["rv64i", "rv64e"] {
            let error = Target::parse(base).expect_err("not implemented");
            assert!(
                matches!(error, TargetError::UnsupportedBase(_)),
                "{base}: {error:?}"
            );
            assert!(error.to_string().contains("not implemented"), "{error}");
        }
    }

    #[test]
    fn rv32e_shares_the_instruction_table_and_limits_registers() {
        let target = Target::parse("rv32e").expect("E is supported");
        assert_eq!(target.name(), "rv32e");
        assert!(target.is_embedded());
        assert_eq!(target.registers(), 16);
        assert!(target.has("i"), "E uses the I instructions");
        assert!(target.allows(lookup("add").expect("add")));

        let with_m = Target::parse("rv32e_m").expect("E plus M");
        assert_eq!(with_m.name(), "rv32em");
        assert!(with_m.allows(lookup("mul").expect("mul")));
        assert_eq!(with_m.registers(), 16);

        assert_eq!(Target::parse("rv32i").expect("i").registers(), 32);
    }

    #[test]
    fn rejects_unknown_base() {
        let error = Target::parse("rv128i").expect_err("no such base");
        assert!(matches!(error, TargetError::UnknownBase(_)));
    }
}
