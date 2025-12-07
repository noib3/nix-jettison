/// A simple, no-allocation parser for the subset of the Cargo.lock format that
/// we need to vendor dependencies.
///
/// This parser only supports the "TOML v0.4" format used by Cargo.lock files
pub(crate) struct CargoLockParser<'lock> {
    /// The byte offset in `src` we're currently at. This is always guaranteed
    /// to be a valid UTF-8 boundary.
    cursor: usize,

    /// The full contents of the Cargo.lock file being parsed.
    src: &'lock str,
}

/// A representation of a `[[package]]` entry in a Cargo.lock file.
///
/// Each entry can contain the following fields:
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
///   section in `Cargo.toml` to override a dependency. For example, this:
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
    name: &'lock str,
    version: &'lock str,
    source: Option<DependencySource<'lock>>,
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum DependencySource<'lock> {
    Registry(RegistrySource<'lock>),
    Git(GitSource<'lock>),
    Path,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct RegistrySource<'lock> {
    pub(crate) checksum: &'lock str,
    pub(crate) kind: RegistryKind<'lock>,
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum RegistryKind<'lock> {
    /// For `crates.io` dependencies we don't need to store the protocol or the
    /// URL since we know to always download them from
    /// `https://static.crates.io/crates`.
    CratesIo,

    /// For other registries we need to store both the protocol and the URL to
    /// know where to download the `config.json` file containing the download
    /// URL template.
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
    url: &'lock str,
    rev: &'lock str,
    branch: Option<&'lock str>,
    tag: Option<&'lock str>,
}
