//! Short codes for versions: the first eight hex characters of the id, longer only when needed.

use super::VersionId;

const MIN_LEN: usize = 8;

/// One code per id, in the same order. Codes that would collide within `ids` grow one
/// character at a time until they differ (or the id has no characters left).
pub fn version_codes(ids: &[VersionId]) -> Vec<String> {
    let suffixes: Vec<&str> =
        ids.iter().map(|id| id.as_str().strip_prefix(VersionId::PREFIX).unwrap_or(id.as_str())).collect();
    let mut lens: Vec<usize> = suffixes.iter().map(|s| s.len().min(MIN_LEN)).collect();
    loop {
        let codes: Vec<&str> = suffixes.iter().zip(&lens).map(|(s, &n)| &s[..n]).collect();
        let mut grew = false;
        for i in 0..codes.len() {
            let collides = codes.iter().enumerate().any(|(j, c)| j != i && *c == codes[i]);
            if collides && lens[i] < suffixes[i].len() {
                lens[i] += 1;
                grew = true;
            }
        }
        if !grew {
            return codes.into_iter().map(str::to_owned).collect();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(suffix: &str) -> VersionId {
        VersionId::from_suffix(suffix)
    }

    #[test]
    fn a_code_is_the_first_eight_hex_characters() {
        assert_eq!(version_codes(&[id("7f3a9c21aaaabbbbccccddddeeeeffff")]), vec!["7f3a9c21"]);
    }

    #[test]
    fn codes_that_would_collide_are_lengthened_until_they_differ() {
        let ids = [
            id("7f3a9c21aa01000000000000000000"),
            id("7f3a9c21ab02000000000000000000"),
            id("0123456789abcdef0123456789abcd"),
        ];
        assert_eq!(version_codes(&ids), vec!["7f3a9c21aa", "7f3a9c21ab", "01234567"]);
    }

    #[test]
    fn a_suffix_shorter_than_eight_is_used_whole() {
        assert_eq!(version_codes(&[id("08"), id("09")]), vec!["08", "09"]);
    }
}
