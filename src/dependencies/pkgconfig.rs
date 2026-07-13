//! Support for pkg-config/pkgconf module versions.
//!
//! pkg-config has no version grammar: any string in a `.pc` file's `Version:`
//! field is a valid version. What it does define is an *ordering*, used to
//! evaluate constraints like `foo >= 1.2`. [`PkgVersion`] wraps such a version
//! string and implements that ordering, and [`PkgConstraint`] pairs it with one
//! of pkg-config's six comparison operators.

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

/// A pkg-config module version.
///
/// This is an opaque string with an ordering, not a structured version: pkgconf
/// accepts anything in a `.pc` file's `Version:` field. Comparison follows
/// `pkgconf_compare_version` (libpkgconf/pkg.c), which pkgconf documents as "RPM
/// version comparison rules as described in the LSB". It is `rpmvercmp` with two
/// deviations, both reproduced here:
///
/// * Versions that differ only in case compare equal (`1.0RC1` == `1.0rc1`),
///   because pkgconf short-circuits on `strcasecmp`. RPM is case-sensitive.
/// * `^` has no special meaning and is an ordinary separator, so `1.0^ == 1.0`.
///   Modern RPM sorts `1.0^` after `1.0`.
///
/// Otherwise the rules are rpmvercmp's: the version is walked as alternating
/// runs of digits and letters, with every other character treated as an
/// interchangeable separator; a digit run outranks a letter run; digit runs
/// compare numerically (leading zeroes stripped) and letter runs bytewise; a
/// version that runs out of segments first sorts lower; and `~` sorts before
/// everything, including the end of the version, so `1.0~rc1 < 1.0`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PkgVersion(String);

impl PkgVersion {
    /// Create a version from a `.pc` `Version:` field value.
    pub fn new(version: &str) -> Self {
        Self(version.to_string())
    }

    /// The version as it appears in the `.pc` file.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PkgVersion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for PkgVersion {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(s))
    }
}

impl From<String> for PkgVersion {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for PkgVersion {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl AsRef<str> for PkgVersion {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl PartialOrd for PkgVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PkgVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        compare(&self.0, &other.0)
    }
}

/// Compare two pkg-config version strings, following `pkgconf_compare_version`.
fn compare(a: &str, b: &str) -> Ordering {
    if a.eq_ignore_ascii_case(b) {
        return Ordering::Equal;
    }

    let mut one = a.as_bytes();
    let mut two = b.as_bytes();

    while !one.is_empty() || !two.is_empty() {
        one = skip_separators(one);
        two = skip_separators(two);

        // `~` sorts before everything, including the end of the version.
        if one.first() == Some(&b'~') || two.first() == Some(&b'~') {
            match (one.first(), two.first()) {
                (Some(b'~'), Some(b'~')) => {
                    one = &one[1..];
                    two = &two[1..];
                    continue;
                }
                (Some(b'~'), _) => return Ordering::Less,
                _ => return Ordering::Greater,
            }
        }

        if one.is_empty() || two.is_empty() {
            break;
        }

        let isnum = one[0].is_ascii_digit();
        let (seg1, rest1) = split_run(one, isnum);
        let (seg2, rest2) = split_run(two, isnum);

        // `one` starts a run of the kind we are looking for, so seg1 is never
        // empty. An empty seg2 means the two versions have different segment
        // kinds here, and a digit run outranks a letter run.
        if seg2.is_empty() {
            return if isnum {
                Ordering::Greater
            } else {
                Ordering::Less
            };
        }

        let ord = if isnum {
            compare_numeric(seg1, seg2)
        } else {
            seg1.cmp(seg2)
        };
        if ord != Ordering::Equal {
            return ord;
        }

        one = rest1;
        two = rest2;
    }

    // The loop only exits with a segment left on at most one side: whichever
    // still has one has more version left, and sorts higher.
    match (one.is_empty(), two.is_empty()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Less,
        (false, _) => Ordering::Greater,
    }
}

/// Skip characters that are neither alphanumeric nor `~`.
fn skip_separators(s: &[u8]) -> &[u8] {
    let end = s
        .iter()
        .position(|c| c.is_ascii_alphanumeric() || *c == b'~')
        .unwrap_or(s.len());
    &s[end..]
}

/// Split off the leading run of digits (if `isnum`) or letters, returning the
/// run and the remainder. The run is empty if `s` does not start with one.
fn split_run(s: &[u8], isnum: bool) -> (&[u8], &[u8]) {
    let pred = if isnum {
        u8::is_ascii_digit
    } else {
        u8::is_ascii_alphabetic
    };
    let end = s.iter().position(|c| !pred(c)).unwrap_or(s.len());
    s.split_at(end)
}

/// Compare two runs of digits by value: strip leading zeroes, then the longer
/// run is larger, and equal-length runs compare bytewise.
fn compare_numeric(a: &[u8], b: &[u8]) -> Ordering {
    let a = strip_leading_zeroes(a);
    let b = strip_leading_zeroes(b);
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

fn strip_leading_zeroes(s: &[u8]) -> &[u8] {
    let start = s.iter().position(|c| *c != b'0').unwrap_or(s.len());
    &s[start..]
}

/// A comparison operator in a pkg-config version constraint.
///
/// These are the six operators pkgconf accepts in a `Requires:` field or on the
/// command line (`pkgconf --exists 'foo >= 1.2'`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PkgComparison {
    /// `<`
    Less,
    /// `<=`
    LessEqual,
    /// `=`
    Equal,
    /// `!=`
    NotEqual,
    /// `>=`
    GreaterEqual,
    /// `>`
    Greater,
}

impl PkgComparison {
    /// The operator as it is spelled in a `.pc` file.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Less => "<",
            Self::LessEqual => "<=",
            Self::Equal => "=",
            Self::NotEqual => "!=",
            Self::GreaterEqual => ">=",
            Self::Greater => ">",
        }
    }

    /// Whether an ordering between a module's version and the constraint's
    /// version satisfies this operator.
    fn matches(&self, ordering: Ordering) -> bool {
        match self {
            Self::Less => ordering == Ordering::Less,
            Self::LessEqual => ordering != Ordering::Greater,
            Self::Equal => ordering == Ordering::Equal,
            Self::NotEqual => ordering != Ordering::Equal,
            Self::GreaterEqual => ordering != Ordering::Less,
            Self::Greater => ordering == Ordering::Greater,
        }
    }
}

impl fmt::Display for PkgComparison {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An error returned when a string is not a pkg-config comparison operator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsePkgComparisonError(String);

impl fmt::Display for ParsePkgComparisonError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid pkg-config comparison operator: {}", self.0)
    }
}

impl std::error::Error for ParsePkgComparisonError {}

impl FromStr for PkgComparison {
    type Err = ParsePkgComparisonError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "<" => Ok(Self::Less),
            "<=" => Ok(Self::LessEqual),
            "=" | "==" => Ok(Self::Equal),
            "!=" => Ok(Self::NotEqual),
            ">=" => Ok(Self::GreaterEqual),
            ">" => Ok(Self::Greater),
            _ => Err(ParsePkgComparisonError(s.to_string())),
        }
    }
}

/// A pkg-config version constraint, e.g. `>= 1.2`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PkgConstraint {
    /// The comparison operator.
    pub comparison: PkgComparison,
    /// The version being compared against.
    pub version: PkgVersion,
}

impl PkgConstraint {
    /// Create a new constraint.
    pub fn new(comparison: PkgComparison, version: PkgVersion) -> Self {
        Self {
            comparison,
            version,
        }
    }

    /// Create a `>=` constraint, the common case.
    pub fn at_least(version: PkgVersion) -> Self {
        Self::new(PkgComparison::GreaterEqual, version)
    }

    /// Whether `version` satisfies this constraint.
    pub fn matches(&self, version: &PkgVersion) -> bool {
        self.comparison.matches(version.cmp(&self.version))
    }
}

impl fmt::Display for PkgConstraint {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}", self.comparison, self.version)
    }
}

/// An error returned when a string is not a pkg-config version constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsePkgConstraintError {
    /// The comparison operator was not recognized.
    Comparison(ParsePkgComparisonError),
    /// The constraint had no version after the operator.
    MissingVersion,
}

impl fmt::Display for ParsePkgConstraintError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Comparison(e) => e.fmt(f),
            Self::MissingVersion => f.write_str("missing version in pkg-config constraint"),
        }
    }
}

impl std::error::Error for ParsePkgConstraintError {}

impl From<ParsePkgComparisonError> for ParsePkgConstraintError {
    fn from(e: ParsePkgComparisonError) -> Self {
        Self::Comparison(e)
    }
}

impl FromStr for PkgConstraint {
    type Err = ParsePkgConstraintError;

    /// Parse a constraint such as `>= 1.2`. Whitespace between the operator and
    /// the version is optional, since meson and `.pc` files both write `>=1.2`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let split = s
            .find(|c: char| !"<>=!".contains(c))
            .ok_or(ParsePkgConstraintError::MissingVersion)?;
        let (comparison, version) = s.split_at(split);
        let version = version.trim();
        if version.is_empty() {
            return Err(ParsePkgConstraintError::MissingVersion);
        }
        Ok(Self::new(comparison.parse()?, PkgVersion::new(version)))
    }
}

/// Convert a pkg-config comparison to the equivalent Debian relation constraint.
///
/// Returns `None` for `!=`, which has no Debian equivalent: expressing it would
/// need a disjunction (`<< v | >> v`).
#[cfg(feature = "debian")]
pub fn pkg_comparison_to_debian(
    comparison: PkgComparison,
) -> Option<debian_control::relations::VersionConstraint> {
    use debian_control::relations::VersionConstraint as Deb;
    Some(match comparison {
        PkgComparison::Less => Deb::LessThan,
        PkgComparison::LessEqual => Deb::LessThanEqual,
        PkgComparison::Equal => Deb::Equal,
        PkgComparison::GreaterEqual => Deb::GreaterThanEqual,
        PkgComparison::Greater => Deb::GreaterThan,
        PkgComparison::NotEqual => return None,
    })
}

/// Convert a pkg-config constraint to the equivalent Debian relation constraint.
///
/// Returns `None` if either the comparison has no Debian equivalent (`!=`) or the
/// version cannot be represented as a Debian version; see
/// [`pkg_comparison_to_debian`] and [`pkg_version_to_debian`].
#[cfg(feature = "debian")]
pub fn pkg_constraint_to_debian(
    constraint: &PkgConstraint,
) -> Option<(
    debian_control::relations::VersionConstraint,
    debversion::Version,
)> {
    Some((
        pkg_comparison_to_debian(constraint.comparison)?,
        pkg_version_to_debian(&constraint.version)?,
    ))
}

/// Convert a pkg-config version to a Debian (upstream) version.
///
/// The pkg-config version is used as the Debian upstream version unchanged, with
/// no epoch and no Debian revision. No rewriting is done: the two orderings agree
/// on `~` (so pre-releases keep sorting below their release), on leading zeroes,
/// on numeric width (`1.9` < `1.10`), and on an alphabetic suffix attached to a
/// digit run, which both sort *above* the bare release (`1.0rc1` > `1.0`).
///
/// They do not agree in every case. Where a whole segment is alphabetic and the
/// segment facing it is numeric, pkgconf ranks the letter run below the digit run
/// while dpkg ranks it above, so pkgconf has `1.a` < `1.0` and dpkg has
/// `1.a` > `1.0`. No pass-through mapping can reconcile that, and rewriting the
/// version would break the cases that do agree, so the divergence is accepted:
/// such versions are vanishingly rare in real `.pc` files, where an alphabetic
/// part is a suffix (`1.0rc1`) rather than a standalone segment.
///
/// The version is built directly rather than parsed, so a `-` in the pkg-config
/// version stays in the upstream part instead of being taken as the start of a
/// Debian revision. The result therefore does not always round-trip through a
/// Debian relation string: `foo (>= 1.0-2)` reparses as upstream `1.0` with
/// revision `2`.
///
/// Returns `None` if the version cannot be represented as a Debian upstream
/// version, which for a Debian upstream version means it does not start with a
/// digit, or contains a character Debian does not allow.
#[cfg(feature = "debian")]
pub fn pkg_version_to_debian(version: &PkgVersion) -> Option<debversion::Version> {
    if !valid_debian_upstream_version(version.as_str()) {
        return None;
    }
    Some(debversion::Version {
        epoch: None,
        upstream_version: version.as_str().to_string(),
        debian_revision: None,
    })
}

/// Whether `s` is valid as the upstream part of a Debian version.
///
/// Policy 5.6.12: an upstream version must start with a digit and may otherwise
/// contain alphanumerics and `.`, `+`, `-`, `:`, `~`. `:` and `-` are only
/// allowed when there is an epoch and a revision respectively; since
/// [`pkg_version_to_debian`] emits neither, both are rejected here -- a `:`
/// would reparse as an epoch separator, and while a `-` is accepted by
/// `debversion` in a revision-less version, it would reparse as a revision.
#[cfg(feature = "debian")]
fn valid_debian_upstream_version(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_digit())
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '~'))
}

/// Convert a Debian version to a pkg-config version.
///
/// This inverts [`pkg_version_to_debian`]: the upstream version is used
/// directly, and the epoch and Debian revision, which have no pkg-config
/// equivalent, are dropped.
#[cfg(feature = "debian")]
pub fn debian_version_to_pkg(version: &debversion::Version) -> PkgVersion {
    PkgVersion::new(&version.upstream_version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_and_parse() {
        let v: PkgVersion = "1.2.3".parse().unwrap();
        assert_eq!(v.to_string(), "1.2.3");
        assert_eq!(v.as_str(), "1.2.3");
    }

    /// Versions in strictly ascending pkgconf order. Every pair is checked, so
    /// each entry must be less than all the entries after it.
    #[rustfmt::skip]
    const ASCENDING: &[&str] = &[
        // `~` sorts before everything, including the end of the version, and
        // nests: the more `~`, the lower.
        "0.9~~",
        "0.9~",
        "0.9",
        // In a given position a letter run ranks below a digit run, so `1.a`
        // sorts below every `1.0*` -- but still above `0.9`, since the leading
        // segment decides first.
        "1.a",
        "1.0~~",
        "1.0~",
        "1.0~a",
        "1.0~rc1",
        "1.0~rc2",
        // A version that runs out of segments first sorts lower.
        "1.0",
        // A bare pre-release tag sorts *after* its release, unlike `~rc1`.
        "1.0a",
        "1.0a1",
        "1.0a2",
        "1.0b",
        "1.0rc1",
        // A further segment outranks none at all.
        "1.0.1",
        "1.0.2",
        // Digit runs compare by value, not bytewise: 9 < 10 < 100.
        "1.9",
        "1.10",
        "1.100",
        "2.0",
        "10.0",
    ];

    #[test]
    fn test_ordering() {
        for (i, lesser) in ASCENDING.iter().enumerate() {
            for greater in &ASCENDING[i + 1..] {
                assert_eq!(
                    PkgVersion::new(lesser).cmp(&PkgVersion::new(greater)),
                    Ordering::Less,
                    "{lesser} should sort before {greater}"
                );
                assert_eq!(
                    PkgVersion::new(greater).cmp(&PkgVersion::new(lesser)),
                    Ordering::Greater,
                    "{greater} should sort after {lesser}"
                );
            }
        }
    }

    /// Versions pkgconf considers equal despite differing as strings.
    const EQUIVALENT: &[(&str, &str)] = &[
        // Every non-alphanumeric is an interchangeable separator, whatever it
        // is, however many there are, and wherever it sits.
        ("1.0.0", "1_0-0"),
        ("1.0.1", "1:0:1"),
        ("1.0", "1..0"),
        ("1.0", "-1.0-"),
        ("1.0a", "1.0-a"),
        ("1.0.", "1.0"),
        // ... including `^`, which RPM (but not pkgconf) treats specially.
        ("1.0^1", "1.0.1"),
        ("1.0^", "1.0"),
        // A separator does not break up a `~` run's meaning either.
        ("1.0~rc1", "1.0~rc-1"),
        // Leading zeroes in a digit run are not significant.
        ("1.01", "1.1"),
        ("1.0007", "1.7"),
        // pkgconf short-circuits on a case-insensitive match, so versions
        // differing only in case are equal. RPM would order these.
        ("1.0RC1", "1.0rc1"),
        ("ALPHA", "alpha"),
    ];

    #[test]
    fn test_equivalent() {
        for (a, b) in EQUIVALENT {
            assert_eq!(
                PkgVersion::new(a).cmp(&PkgVersion::new(b)),
                Ordering::Equal,
                "{a} should compare equal to {b}"
            );
        }
    }

    #[test]
    fn test_equal_to_itself() {
        for v in ASCENDING {
            assert_eq!(
                PkgVersion::new(v).cmp(&PkgVersion::new(v)),
                Ordering::Equal,
                "{v} should compare equal to itself"
            );
        }
    }

    #[test]
    fn test_sort() {
        let mut versions = vec![
            PkgVersion::new("1.10"),
            PkgVersion::new("1.0~rc1"),
            PkgVersion::new("1.9"),
            PkgVersion::new("1.0"),
        ];
        versions.sort();
        assert_eq!(
            versions,
            vec![
                PkgVersion::new("1.0~rc1"),
                PkgVersion::new("1.0"),
                PkgVersion::new("1.9"),
                PkgVersion::new("1.10"),
            ]
        );
    }

    #[test]
    fn test_parse_constraint() {
        for (text, comparison, version) in [
            (">= 1.2", PkgComparison::GreaterEqual, "1.2"),
            // meson and .pc files both write these without a space.
            (">=1.2", PkgComparison::GreaterEqual, "1.2"),
            ("> 1.2", PkgComparison::Greater, "1.2"),
            ("<2.0", PkgComparison::Less, "2.0"),
            ("<= 2.0", PkgComparison::LessEqual, "2.0"),
            ("= 1.4", PkgComparison::Equal, "1.4"),
            ("== 1.4", PkgComparison::Equal, "1.4"),
            ("!= 1.4", PkgComparison::NotEqual, "1.4"),
            ("  >=  1.2  ", PkgComparison::GreaterEqual, "1.2"),
        ] {
            let constraint: PkgConstraint = text.parse().unwrap();
            assert_eq!(
                constraint,
                PkgConstraint::new(comparison, PkgVersion::new(version)),
                "parsing {text}"
            );
        }
    }

    #[test]
    fn test_parse_constraint_errors() {
        assert_eq!(
            ">= ".parse::<PkgConstraint>(),
            Err(ParsePkgConstraintError::MissingVersion)
        );
        assert_eq!(
            ">=".parse::<PkgConstraint>(),
            Err(ParsePkgConstraintError::MissingVersion)
        );
        assert_eq!(
            "=> 1.2".parse::<PkgConstraint>(),
            Err(ParsePkgConstraintError::Comparison(
                ParsePkgComparisonError("=>".to_string())
            ))
        );
        // A bare version is not a constraint.
        assert_eq!(
            "1.2".parse::<PkgConstraint>(),
            Err(ParsePkgConstraintError::Comparison(
                ParsePkgComparisonError("".to_string())
            ))
        );
    }

    #[test]
    fn test_constraint_display() {
        let constraint: PkgConstraint = ">=1.2".parse().unwrap();
        assert_eq!(constraint.to_string(), ">= 1.2");
    }

    #[test]
    fn test_constraint_matches() {
        let cases = [
            (">= 1.2", "1.2", true),
            (">= 1.2", "1.3", true),
            (">= 1.2", "1.1", false),
            ("> 1.2", "1.2", false),
            ("> 1.2", "1.3", true),
            ("< 2.0", "1.9", true),
            ("< 2.0", "2.0", false),
            ("<= 2.0", "2.0", true),
            ("<= 2.0", "2.1", false),
            ("= 1.4", "1.4", true),
            ("= 1.4", "1.5", false),
            ("!= 1.4", "1.4", false),
            ("!= 1.4", "1.5", true),
            // Pre-releases sort below the release they lead up to.
            (">= 1.0", "1.0~rc1", false),
            ("< 1.0", "1.0~rc1", true),
        ];
        for (text, version, expected) in cases {
            let constraint: PkgConstraint = text.parse().unwrap();
            assert_eq!(
                constraint.matches(&PkgVersion::new(version)),
                expected,
                "{version} against {text}"
            );
        }
    }

    #[cfg(feature = "debian")]
    #[test]
    fn test_pkg_constraint_to_debian() {
        use debian_control::relations::VersionConstraint as Deb;
        for (text, expected) in [
            (">= 1.2", Some(Deb::GreaterThanEqual)),
            ("> 1.2", Some(Deb::GreaterThan)),
            ("<= 1.2", Some(Deb::LessThanEqual)),
            ("< 1.2", Some(Deb::LessThan)),
            ("= 1.2", Some(Deb::Equal)),
            // Debian relations have no `!=`.
            ("!= 1.2", None),
        ] {
            let constraint: PkgConstraint = text.parse().unwrap();
            let converted = pkg_constraint_to_debian(&constraint);
            assert_eq!(
                converted.map(|(comparison, _)| comparison),
                expected,
                "converting {text}"
            );
        }
    }

    #[cfg(feature = "debian")]
    #[test]
    fn test_pkg_constraint_to_debian_unrepresentable_version() {
        // The comparison converts, but the version does not.
        let constraint: PkgConstraint = ">= 1_0".parse().unwrap();
        assert_eq!(pkg_constraint_to_debian(&constraint), None);
    }

    #[cfg(feature = "debian")]
    #[test]
    fn test_pkg_version_to_debian() {
        for (pkg, deb) in [("1.0", "1.0"), ("1.2.3", "1.2.3"), ("1.0~rc1", "1.0~rc1")] {
            let version = pkg_version_to_debian(&PkgVersion::new(pkg)).unwrap();
            assert_eq!(version.to_string(), deb);
        }
    }

    #[cfg(feature = "debian")]
    #[test]
    fn test_pkg_version_to_debian_unrepresentable() {
        // Debian upstream versions must start with a digit ...
        assert_eq!(pkg_version_to_debian(&PkgVersion::new("v1.0")), None);
        assert_eq!(pkg_version_to_debian(&PkgVersion::new("")), None);
        // ... and `-` and `:` would reparse as a revision or an epoch.
        assert_eq!(pkg_version_to_debian(&PkgVersion::new("1.0-2")), None);
        assert_eq!(pkg_version_to_debian(&PkgVersion::new("1:0")), None);
        // Underscores are not valid in a Debian version at all.
        assert_eq!(pkg_version_to_debian(&PkgVersion::new("1_0")), None);
    }

    #[cfg(feature = "debian")]
    #[test]
    fn test_debian_version_to_pkg() {
        // The epoch and Debian revision have no pkg-config equivalent.
        let debian: debversion::Version = "2:1.2.3-1".parse().unwrap();
        assert_eq!(debian_version_to_pkg(&debian), PkgVersion::new("1.2.3"));
    }

    #[cfg(feature = "debian")]
    #[test]
    fn test_debian_round_trip() {
        for pkg in ["1.0", "1.2.3", "1.0~rc1", "1.0+dfsg"] {
            let version = PkgVersion::new(pkg);
            let debian = pkg_version_to_debian(&version).unwrap();
            assert_eq!(debian_version_to_pkg(&debian), version);
        }
    }

    /// pkgconf ranks a letter run below a digit run in the same position; dpkg
    /// ranks it above. A pass-through conversion cannot reconcile that, so the
    /// orderings diverge on versions with a standalone alphabetic segment.
    ///
    /// This is the one exception to test_debian_ordering_agrees below, and is
    /// pinned here so it cannot change unnoticed. See pkg_version_to_debian.
    #[cfg(feature = "debian")]
    #[test]
    fn test_debian_ordering_diverges_on_alpha_segment() {
        let alpha = pkg_version_to_debian(&PkgVersion::new("1.a")).unwrap();
        let numeric = pkg_version_to_debian(&PkgVersion::new("1.0")).unwrap();

        assert_eq!(
            PkgVersion::new("1.a").cmp(&PkgVersion::new("1.0")),
            Ordering::Less,
            "pkgconf sorts a letter run below a digit run"
        );
        assert!(alpha > numeric, "dpkg sorts it above");
    }

    /// Apart from the standalone-alphabetic-segment case above, the pkgconf
    /// ordering survives the conversion, so a constraint keeps its meaning.
    #[cfg(feature = "debian")]
    #[test]
    fn test_debian_ordering_agrees() {
        let convertible: Vec<&str> = ASCENDING
            .iter()
            .copied()
            .filter(|v| pkg_version_to_debian(&PkgVersion::new(v)).is_some())
            // The known divergence, asserted separately above.
            .filter(|v| *v != "1.a")
            .collect();
        for (i, lesser) in convertible.iter().enumerate() {
            for greater in &convertible[i + 1..] {
                let lesser = pkg_version_to_debian(&PkgVersion::new(lesser)).unwrap();
                let greater = pkg_version_to_debian(&PkgVersion::new(greater)).unwrap();
                assert!(lesser < greater, "{lesser} should sort before {greater}");
            }
        }
    }
}
