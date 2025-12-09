use core::fmt;
use std::borrow::Cow;

use compact_str::CompactString;

const CRATES_IO_REGISTRY_URL: &str = cargo::sources::CRATES_IO_INDEX;
const CRATES_IO_SPARSE_URL: &str = "https://index.crates.io/";

/// A simple, no-allocation parser for the subset of the Cargo.lock format that
/// we need to vendor dependencies.
pub(crate) struct CargoLockParser<'lock> {
    /// The byte offset in `src` we're currently at. This is always guaranteed
    /// to be a valid UTF-8 boundary.
    cursor_offset: usize,

    /// The semantic position of the cursor.
    cursor_position: CursorPosition,

    /// The full contents of the Cargo.lock file being parsed.
    src: &'lock str,
}

/// A representation of a `[[package]]` entry in a Cargo.lock file.
///
/// Each entry can contain the following fields (in this order):
///
/// - `name`: required;
///
/// - `version`: required;
///
/// - `source`: optional, not present for path dependencies;
///
/// - `checksum`: optional, only present for dependencies from registries (like
///   crates.io);
///
/// - `dependencies` OR `replace`: both of them are optional, and they're
///   mutually exclusive. `replace` is present when using the `[replace]`
///   section in a `Cargo.toml` to override a dependency. For example, this:
///
///   ```toml
///   [replace]
///   "serde:0.8.0" = { path = "my/serde" }
///   ```
///
///   would result in two `Cargo.lock` entries:
///
///   ```toml
///   [[package]]
///   name = "serde"
///   version = "0.8.0"
///   source = "registry+https://github.com/rust-lang/crates.io-index"
///   replace = "serde 0.8.0"   # Points to the replacement
///
///   # The replacement package (what's actually used).
///   [[package]]
///   name = "serde"
///   version = "0.8.0"
///   ... other fields ...
///   ```
///
///   Only the replacement package is actually compiled, so we skip entries
///   with a `replace` field. We also always skip parsing the `dependencies`
///   field since we don't need it for vendoring.
pub(crate) struct PackageEntry<'lock> {
    pub(crate) name: &'lock str,
    pub(crate) version: &'lock str,
    pub(crate) source: Option<PackageSource<'lock>>,
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum PackageSource<'lock> {
    Registry(RegistrySource<'lock>),
    Git(GitSource<'lock>),
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct RegistrySource<'lock> {
    pub(crate) checksum: &'lock str,
    pub(crate) kind: RegistryKind<'lock>,
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum RegistryKind<'lock> {
    /// For `crates.io` dependencies we don't need to store the protocol or the
    /// URL since we know to always download crates from
    /// `https://static.crates.io/crates`.
    CratesIo,

    /// For other registries we need to store both the protocol and the URL to
    /// know where to get the `config.json` file containing the download URL
    /// template.
    #[expect(unused, reason = "we can't handle custom registries yet")]
    Other { protocol: RegistryProtocol, url: &'lock str },
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum RegistryProtocol {
    /// Git-based index protocol.
    Registry,

    /// HTTP-based sparse index protocol.
    Sparse,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct GitSource<'lock> {
    pub(crate) url: &'lock str,
    pub(crate) rev: &'lock str,
    pub(crate) r#ref: Option<GitSourceRef<'lock>>,
}

/// See [docs] for more infos.
///
/// These were made mutually exclusive in [8984].
///
/// [docs]: https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#choice-of-commit
/// [8984]: https://github.com/rust-lang/cargo/pull/8984
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum GitSourceRef<'lock> {
    Branch(&'lock str),
    Tag(&'lock str),
    Rev(&'lock str),
}

/// The type of error that can occur when parsing the contents of a
/// `Cargo.lock` file.
#[derive(Debug, derive_more::Display, cauchy::Error)]
pub(crate) enum CargoLockParseError {
    #[display(
        "invalid source protocol: {protocol:?}, expected one of 'registry+', \
         'sparse+' or 'git+'"
    )]
    InvalidSourceProtocol { protocol: CompactString },

    #[display(
        "invalid git ref key: {key:?}, expected one of 'branch', 'tag' or \
         'rev'"
    )]
    InvalidGitSourceRefKey { key: CompactString },

    #[display("missing closing quote for field {field_name:?}")]
    MissingClosingQuote { field_name: &'static str },

    #[display("expected field {field_name:?} after {after:?}")]
    MissingField { field_name: &'static str, after: &'static str },

    #[display("missing git revision in {source:?}")]
    MissingGitRevision { source: CompactString },

    #[display("missing source protocol in {source:?}")]
    MissingSourceProtocol { source: CompactString },
}

trait Search<Needle>: AsRef<str> {
    /// Returns the byte offset of the first occurrence of `needle` in `self`,
    /// or `None` if `needle` is not found.
    fn search(&self, needle: Needle) -> Option<usize>;
}

enum CursorPosition {
    StartOfEntry,
    EndOfName,
    EndOfVersion,
    EndOfSource,
    EndOfChecksum,
    EndOfFile,
}

impl<'lock> CargoLockParser<'lock> {
    pub(crate) fn new(cargo_dot_lock: &'lock str) -> Self {
        let (cursor_offset, cursor_position) = match cargo_dot_lock.search("[[")
        {
            Some(offset) => (offset, CursorPosition::StartOfEntry),
            None => (0, CursorPosition::EndOfFile),
        };

        Self { cursor_offset, cursor_position, src: cargo_dot_lock }
    }

    #[allow(clippy::too_many_lines)]
    #[inline]
    fn next_inner(
        &mut self,
    ) -> Result<Option<PackageEntry<'lock>>, CargoLockParseError> {
        // Start with a dummy entry, we'll fill its fields as we parse them.
        let mut entry = PackageEntry { name: "", version: "", source: None };

        loop {
            match self.cursor_position {
                CursorPosition::StartOfEntry => {
                    let expected = "[[package]]\nname = \"";
                    if !self.src_after_cursor().starts_with(expected) {
                        return Err(CargoLockParseError::MissingField {
                            field_name: "name",
                            after: "[[package]]",
                        });
                    }
                    self.cursor_offset += expected.len();
                    let name_start = self.cursor_offset;
                    let Some(name_end) = self.search_from_cursor(b'"') else {
                        return Err(CargoLockParseError::MissingClosingQuote {
                            field_name: "name",
                        });
                    };
                    entry.name = &self.src[name_start..name_end];
                    self.cursor_offset = name_end + 1;
                    self.cursor_position = CursorPosition::EndOfName;
                },

                CursorPosition::EndOfName => {
                    let expected = "\nversion = \"";
                    if !self.src_after_cursor().starts_with(expected) {
                        return Err(CargoLockParseError::MissingField {
                            field_name: "version",
                            after: "name",
                        });
                    }
                    self.cursor_offset += expected.len();
                    let version_start = self.cursor_offset;
                    let version_end = self.search_from_cursor(b'"').ok_or(
                        CargoLockParseError::MissingClosingQuote {
                            field_name: "version",
                        },
                    )?;
                    entry.version = &self.src[version_start..version_end];
                    self.cursor_offset = version_end + 1;
                    self.cursor_position = CursorPosition::EndOfVersion;
                },

                CursorPosition::EndOfVersion => {
                    let expected = "\nsource = \"";
                    if !self.src_after_cursor().starts_with(expected) {
                        entry.source = None;
                        self.cursor_position = CursorPosition::EndOfChecksum;
                        continue;
                    }
                    self.cursor_offset += expected.len();
                    let source_start = self.cursor_offset;
                    let source_end = self.search_from_cursor(b'"').ok_or(
                        CargoLockParseError::MissingClosingQuote {
                            field_name: "source",
                        },
                    )?;
                    let source = &self.src[source_start..source_end];
                    entry.source = Some(PackageSource::parse(source)?);
                    self.cursor_offset = source_end + 1;
                    self.cursor_position = CursorPosition::EndOfSource;
                },

                CursorPosition::EndOfSource => {
                    let Some(PackageSource::Registry(src)) = &mut entry.source
                    else {
                        self.cursor_position = CursorPosition::EndOfChecksum;
                        continue;
                    };
                    let expected = "\nchecksum = \"";
                    if !self.src_after_cursor().starts_with(expected) {
                        return Err(CargoLockParseError::MissingField {
                            field_name: "checksum",
                            after: "source",
                        });
                    }
                    self.cursor_offset += expected.len();
                    let checksum_start = self.cursor_offset;
                    let checksum_end = self.search_from_cursor(b'"').ok_or(
                        CargoLockParseError::MissingClosingQuote {
                            field_name: "checksum",
                        },
                    )?;
                    src.checksum = &self.src[checksum_start..checksum_end];
                    self.cursor_offset = checksum_end + 1;
                    self.cursor_position = CursorPosition::EndOfChecksum;
                },

                CursorPosition::EndOfChecksum => {
                    if !self.src_after_cursor().starts_with('\n') {
                        self.cursor_position = CursorPosition::EndOfFile;
                        break;
                    }
                    self.cursor_offset += 1;
                    let skip_entry = self.src_after_cursor().starts_with('r');
                    match self.search_from_cursor("[[") {
                        Some(offset) => {
                            self.cursor_offset = offset;
                            self.cursor_position = CursorPosition::StartOfEntry;
                        },
                        None => {
                            self.cursor_position = CursorPosition::EndOfFile;
                        },
                    }
                    if !skip_entry {
                        break;
                    }
                },

                CursorPosition::EndOfFile => return Ok(None),
            }
        }

        Ok(Some(entry))
    }

    /// Searches for `needle` in the source string starting from the current
    /// cursor position, returning the absolute byte offset if found (from the
    /// start of [`src`](Self::src), not from the cursor).
    #[inline]
    fn search_from_cursor<T>(&self, needle: T) -> Option<usize>
    where
        &'lock str: Search<T>,
    {
        self.src_after_cursor()
            .search(needle)
            .map(|offset| offset + self.cursor_offset)
    }

    /// Returns the portion of the source string after the current cursor
    /// position.
    #[inline]
    fn src_after_cursor(&self) -> &'lock str {
        &self.src[self.cursor_offset..]
    }
}

impl<'lock> PackageSource<'lock> {
    /// Parses a package source string into a [`PackageSource`].
    ///
    /// Note that the [`checksum`](RegistrySource::checksum) field is left
    /// empty. The caller is responsible for setting it.
    #[inline]
    fn parse(source: &'lock str) -> Result<Self, CargoLockParseError> {
        let (protocol, url) = source.split_once('+').ok_or_else(|| {
            CargoLockParseError::MissingSourceProtocol { source: source.into() }
        })?;

        let protocol = match protocol {
            "registry" => RegistryProtocol::Registry,
            "sparse" => RegistryProtocol::Sparse,
            "git" => return GitSource::try_from(url).map(Self::Git),
            _ => {
                return Err(CargoLockParseError::InvalidSourceProtocol {
                    protocol: protocol.into(),
                });
            },
        };

        let kind = match protocol {
            RegistryProtocol::Registry if url == CRATES_IO_REGISTRY_URL => {
                RegistryKind::CratesIo
            },
            RegistryProtocol::Sparse if url == CRATES_IO_SPARSE_URL => {
                RegistryKind::CratesIo
            },
            _ => RegistryKind::Other { protocol, url },
        };

        Ok(Self::Registry(RegistrySource { checksum: "", kind }))
    }
}

impl GitSource<'_> {
    /// Returns a value that formats `self` as a Cargo config entry that
    /// replaces this git source with the given `replace_with` source.
    ///
    /// The returned value can be appended to a `config.toml` file used by
    /// Cargo.
    pub(crate) fn into_cargo_config_entry(
        self,
        replace_with: &str,
    ) -> impl fmt::Display {
        struct GitSourceConfigEntry<'lock, 'vendor> {
            source: GitSource<'lock>,
            replace_with: &'vendor str,
        }

        impl fmt::Display for GitSourceConfigEntry<'_, '_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let Self { source, replace_with } = self;

                // The format of the `[source."..."]` key doesn't actually
                // matter, as long as it's unique. Cargo uses the URL and the
                // ref to identify the source.
                f.write_str("[source.\"")?;
                f.write_str(source.url)?;
                if let Some(r#ref) = source.r#ref {
                    let (key, value) = r#ref.as_key_value();
                    write!(f, "?{}={}", key, value)?;
                }
                f.write_str("\"]\n")?;

                writeln!(f, "git = \"{}\"", source.url)?;

                if let Some(r#ref) = source.r#ref {
                    let (key, value) = r#ref.as_key_value();
                    writeln!(f, "{} = \"{}\"", key, value)?;
                }

                writeln!(f, "replace-with = \"{}\"", replace_with)
            }
        }

        GitSourceConfigEntry { source: self, replace_with }
    }
}

impl<'lock> GitSourceRef<'lock> {
    /// Converts `self` into the string to pass as the `ref` argument to
    /// `builtins.fetchGit`.
    pub(crate) fn format_for_fetch_git(self) -> Option<Cow<'lock, str>> {
        match self {
            Self::Branch(branch) => Some(Cow::Borrowed(branch)),
            Self::Tag(tag) => Some(Cow::Owned(format!("refs/tags/{tag}"))),
            Self::Rev(rev) => {
                let rev = percent_encoding::percent_decode_str(rev)
                    .decode_utf8()
                    .expect("gave a &str as input, so it must be valid UTF-8");

                rev.starts_with("refs/").then_some(rev)
            },
        }
    }

    fn as_key_value(self) -> (&'static str, &'lock str) {
        match self {
            Self::Branch(branch) => ("branch", branch),
            Self::Tag(tag) => ("tag", tag),
            Self::Rev(rev) => ("rev", rev),
        }
    }
}

impl<'lock> Iterator for CargoLockParser<'lock> {
    type Item = Result<PackageEntry<'lock>, CargoLockParseError>;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        self.next_inner().transpose()
    }
}

impl<'lock> TryFrom<&'lock str> for GitSource<'lock> {
    type Error = CargoLockParseError;

    #[inline]
    fn try_from(source: &'lock str) -> Result<Self, Self::Error> {
        // Git sources have the format: <url>[?<key>=<value>]#<revision>

        let (url_with_query, rev) =
            source.split_once('#').ok_or_else(|| {
                CargoLockParseError::MissingGitRevision {
                    source: source.into(),
                }
            })?;

        let (url, query) =
            url_with_query.split_once('?').unwrap_or((url_with_query, ""));

        let r#ref = if let Some((key, value)) = query.split_once('=') {
            Some(match key {
                "branch" => GitSourceRef::Branch(value),
                "tag" => GitSourceRef::Tag(value),
                "rev" => GitSourceRef::Rev(value),
                _ => {
                    return Err(CargoLockParseError::InvalidGitSourceRefKey {
                        key: key.into(),
                    });
                },
            })
        } else {
            None
        };

        Ok(Self { url, rev, r#ref })
    }
}

impl Search<u8> for &str {
    #[inline]
    fn search(&self, needle: u8) -> Option<usize> {
        memchr::memchr(needle, self.as_bytes())
    }
}

impl Search<&str> for &str {
    #[inline]
    fn search(&self, needle: &str) -> Option<usize> {
        memchr::memmem::find(self.as_bytes(), needle.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple() {
        let source = "https://github.com/user/repo.git#abc123";
        let git_src = GitSource::try_from(source).unwrap();
        assert_eq!(git_src.url, "https://github.com/user/repo.git");
        assert_eq!(git_src.rev, "abc123");
        assert_eq!(git_src.r#ref, None);
    }

    #[test]
    fn with_tag() {
        let source = "https://github.com/user/repo.git?tag=v1.0.0#abc123";
        let git_src = GitSource::try_from(source).unwrap();
        assert_eq!(git_src.url, "https://github.com/user/repo.git");
        assert_eq!(git_src.rev, "abc123");
        assert_eq!(git_src.r#ref, Some(GitSourceRef::Tag("v1.0.0")));
    }

    #[test]
    fn with_branch() {
        let source = "https://github.com/user/repo.git?branch=main#abc123";
        let git_src = GitSource::try_from(source).unwrap();
        assert_eq!(git_src.url, "https://github.com/user/repo.git");
        assert_eq!(git_src.rev, "abc123");
        assert_eq!(git_src.r#ref, Some(GitSourceRef::Branch("main")));
    }
}
