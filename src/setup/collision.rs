//! Collision analysis for setup candidates.
//!
//! `SET-011`: search textual regions in readable regular files no larger than 16
//! MiB under the current selected project root. Use the discovery exclusions,
//! include ignored files, exclude every equal-value alias source file, never
//! follow symlinks, and skip special files. Occurrences are counted as
//! non-overlapping exact byte matches from left to right.
//!
//! `SET-012`: report counts and sanitized relative filenames only, never values,
//! matched lines, or snippets. Findings are advisory (`DIA-004`), so skipped
//! files need not be reported.

use std::io::Read;
use std::path::{Path, PathBuf};

use aho_corasick::{Anchored, automaton::Automaton, nfa::noncontiguous::NFA};

use crate::sanitize;
use crate::setup::discovery::EXCLUDED_DIRECTORIES;

const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
const READ_BUFFER_BYTES: usize = 64 * 1024;

/// Where a candidate value also occurs inside the project.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Collisions {
    pub total: usize,
    /// Sanitized project-relative filenames with their occurrence counts.
    pub files: Vec<(String, usize)>,
}

impl Collisions {
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// One-line advisory summary. It contains no value or matched text.
    pub fn describe(&self) -> String {
        let files: Vec<String> = self
            .files
            .iter()
            .take(3)
            .map(|(name, count)| format!("{name} x{count}"))
            .collect();
        let more = self.files.len().saturating_sub(files.len());
        let suffix = if more > 0 {
            format!(", and {more} more")
        } else {
            String::new()
        };
        format!(
            "{} occurrence(s) elsewhere in this project ({}{suffix})",
            self.total,
            files.join(", ")
        )
    }
}

/// One value to search for, with the files that must be excluded from its search.
pub struct Subject<'a> {
    pub value: &'a str,
    /// Every known equal-value source file, excluded in full (`SET-011`).
    pub source_files: &'a [PathBuf],
}

struct PatternSet {
    matcher: NFA,
    subject_patterns: Vec<Option<usize>>,
}

/// Counts occurrences of every subject under `project_root`.
///
/// The tree is walked once and one multi-pattern matcher scans each eligible
/// textual region.
pub fn analyze(project_root: &Path, subjects: &[Subject<'_>]) -> Vec<Collisions> {
    let mut results = vec![Collisions::default(); subjects.len()];
    if subjects.is_empty() {
        return results;
    }
    let mut patterns = Vec::new();
    let subject_patterns: Vec<Option<usize>> = subjects
        .iter()
        .map(|subject| {
            if subject.value.is_empty() || subject.value.len() as u64 > MAX_FILE_BYTES {
                return None;
            }
            if let Some(index) = patterns
                .iter()
                .position(|value: &&str| *value == subject.value)
            {
                Some(index)
            } else {
                patterns.push(subject.value);
                Some(patterns.len() - 1)
            }
        })
        .collect();
    if patterns.is_empty() {
        return results;
    }
    let Ok(matcher) = NFA::new(&patterns) else {
        return results;
    };
    let pattern_set = PatternSet {
        matcher,
        subject_patterns,
    };
    let canonical_sources: Vec<Vec<PathBuf>> = subjects
        .iter()
        .map(|subject| {
            subject
                .source_files
                .iter()
                .filter_map(|path| path.canonicalize().ok())
                .collect()
        })
        .collect();
    scan(
        project_root,
        project_root,
        subjects,
        &canonical_sources,
        &pattern_set,
        &mut results,
    );
    for collisions in &mut results {
        collisions
            .files
            .sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
    }
    results
}

fn scan(
    root: &Path,
    directory: &Path,
    subjects: &[Subject<'_>],
    canonical_sources: &[Vec<PathBuf>],
    pattern_set: &PatternSet,
    results: &mut [Collisions],
) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            let excluded = entry
                .file_name()
                .to_str()
                .is_some_and(|name| EXCLUDED_DIRECTORIES.contains(&name));
            if !excluded {
                scan(
                    root,
                    &path,
                    subjects,
                    canonical_sources,
                    pattern_set,
                    results,
                );
            }
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let Ok(mut file) = std::fs::File::open(&path) else {
            // Unreadable files are skipped; analysis is advisory.
            continue;
        };
        let Ok(opened_metadata) = file.metadata() else {
            continue;
        };
        if !opened_metadata.is_file() || opened_metadata.len() > MAX_FILE_BYTES {
            continue;
        }
        let Some(pattern_counts) = scan_file(&mut file, &pattern_set.matcher) else {
            continue;
        };
        let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        let canonical_path = path.canonicalize().ok();
        for (index, subject) in subjects.iter().enumerate() {
            if subject.source_files.iter().any(|source| source == &path)
                || canonical_path
                    .as_ref()
                    .is_some_and(|path| canonical_sources[index].contains(path))
            {
                continue;
            }
            let Some(pattern) = pattern_set.subject_patterns[index] else {
                continue;
            };
            let count = pattern_counts[pattern];
            if count > 0 {
                results[index].total += count;
                results[index]
                    .files
                    .push((sanitize::path(&relative), count));
            }
        }
    }
}

fn scan_file(file: &mut std::fs::File, matcher: &NFA) -> Option<Vec<usize>> {
    let pattern_count = matcher.patterns_len();
    let start_state = matcher.start_state(Anchored::No).ok()?;
    let mut state = start_state;
    let mut counts = vec![0; pattern_count];
    let mut next_allowed = vec![0; pattern_count];
    let mut region_len = 0_usize;
    {
        let mut process = |bytes: Option<&[u8]>| {
            let Some(bytes) = bytes else {
                state = start_state;
                region_len = 0;
                next_allowed.fill(0);
                return;
            };
            for &byte in bytes {
                if byte == 0 {
                    state = start_state;
                    region_len = 0;
                    next_allowed.fill(0);
                    continue;
                }
                state = matcher.next_state(Anchored::No, state, byte);
                region_len += 1;
                if !matcher.is_match(state) {
                    continue;
                }
                for index in 0..matcher.match_len(state) {
                    let pattern = matcher.match_pattern(state, index);
                    let pattern_index = pattern.as_usize();
                    let start = region_len - matcher.pattern_len(pattern);
                    if start >= next_allowed[pattern_index] {
                        counts[pattern_index] += 1;
                        next_allowed[pattern_index] = region_len;
                    }
                }
            }
        };
        let mut reader = file.take(MAX_FILE_BYTES + 1);
        let mut buffer = vec![0; READ_BUFFER_BYTES];
        let mut pending_utf8 = Vec::with_capacity(READ_BUFFER_BYTES + 3);
        let mut total = 0_u64;

        loop {
            let read = match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => read,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return None,
            };
            total += read as u64;
            if total > MAX_FILE_BYTES {
                return None;
            }
            pending_utf8.extend_from_slice(&buffer[..read]);
            consume_utf8(&mut pending_utf8, &mut process);
        }

        if !pending_utf8.is_empty() {
            process(None);
        }
    }
    Some(counts)
}

fn consume_utf8(bytes: &mut Vec<u8>, process: &mut impl FnMut(Option<&[u8]>)) {
    let mut consumed = 0;
    loop {
        match std::str::from_utf8(&bytes[consumed..]) {
            Ok(_) => {
                process(Some(&bytes[consumed..]));
                consumed = bytes.len();
                break;
            }
            Err(error) => {
                let valid_end = consumed + error.valid_up_to();
                process(Some(&bytes[consumed..valid_end]));
                let Some(error_len) = error.error_len() else {
                    consumed = valid_end;
                    break;
                };
                process(None);
                consumed = valid_end + error_len;
            }
        }
        if consumed == bytes.len() {
            break;
        }
    }
    if consumed > 0 {
        bytes.drain(..consumed);
    }
}

/// Counts non-overlapping occurrences from left to right.
#[cfg(test)]
fn count_occurrences(haystack: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() || needle.len() > haystack.len() {
        return 0;
    }
    let mut count = 0;
    let mut position = 0;
    while position + needle.len() <= haystack.len() {
        if &haystack[position..position + needle.len()] == needle {
            count += 1;
            position += needle.len();
        } else {
            position += 1;
        }
    }
    count
}

/// Convenience wrapper for one value.
pub fn analyze_one(project_root: &Path, value: &str, source_file: Option<&Path>) -> Collisions {
    let source_files: Vec<PathBuf> = source_file.map(Path::to_path_buf).into_iter().collect();
    analyze(
        project_root,
        &[Subject {
            value,
            source_files: &source_files,
        }],
    )
    .pop()
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Canary;

    struct Tree {
        root: PathBuf,
    }

    impl Tree {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "contextveil-collision-{}-{}",
                std::process::id(),
                Canary::generate("TREE").token()
            ));
            std::fs::create_dir_all(&root).expect("fixture root");
            Self { root }
        }

        fn file(&self, relative: &str, contents: &[u8]) -> PathBuf {
            let path = self.root.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("fixture directories");
            }
            std::fs::write(&path, contents).expect("write fixture file");
            path
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn occurrences_are_counted_across_the_project() {
        let tree = Tree::new();
        tree.file("a.txt", b"value value");
        tree.file("nested/b.txt", b"prefix-value");
        tree.file("c.txt", b"nothing here");

        let collisions = analyze_one(&tree.root, "value", None);
        assert_eq!(collisions.total, 3);
        assert_eq!(collisions.files.len(), 2);
    }

    #[test]
    fn counting_is_non_overlapping_and_left_to_right() {
        assert_eq!(count_occurrences(b"aaaa", b"aa"), 2);
        assert_eq!(count_occurrences(b"aaaaa", b"aa"), 2);
        assert_eq!(count_occurrences(b"abcabc", b"abc"), 2);
        assert_eq!(count_occurrences(b"", b"a"), 0);
        assert_eq!(count_occurrences(b"a", b""), 0);
    }

    #[test]
    fn the_candidates_own_source_file_is_excluded_entirely() {
        let tree = Tree::new();
        let source = tree.file(".env", b"TOKEN=value\nOTHER=value\n");
        tree.file("elsewhere.txt", b"value");

        let with_exclusion = analyze_one(&tree.root, "value", Some(&source));
        assert_eq!(with_exclusion.total, 1);

        let without_exclusion = analyze_one(&tree.root, "value", None);
        assert_eq!(without_exclusion.total, 3);
    }

    #[test]
    fn every_equal_value_alias_file_is_excluded() {
        let tree = Tree::new();
        let first = tree.file(".env", b"TOKEN=value\n");
        let second = tree.file("config/auth.json", br#"{"token":"value"}"#);
        tree.file("README.md", b"value");
        let source_files = vec![first, second];

        let collisions = analyze(
            &tree.root,
            &[Subject {
                value: "value",
                source_files: &source_files,
            }],
        );

        assert_eq!(collisions[0].total, 1);
        assert_eq!(collisions[0].files[0].0, "README.md");
    }

    #[test]
    #[cfg(unix)]
    fn a_symlinked_source_excludes_its_regular_project_target() {
        let tree = Tree::new();
        let target = tree.file("config/auth.json", br#"{"token":"value"}"#);
        let source = tree.root.join("machine-auth.json");
        std::os::unix::fs::symlink(&target, &source).expect("source symlink");

        let collisions = analyze_one(&tree.root, "value", Some(&source));

        assert!(collisions.is_empty());
    }

    #[test]
    fn binary_files_scan_only_complete_utf8_regions() {
        let tree = Tree::new();
        tree.file(
            "blob.bin",
            &[
                0x00, 0xff, b'v', b'a', b'l', 0xfe, b'v', b'a', 0x00, b'l', 0xff, b'v', b'a', b'l',
                0x00,
            ],
        );
        let collisions = analyze_one(&tree.root, "val", None);
        assert_eq!(collisions.total, 2);
    }

    #[test]
    fn matches_can_cross_read_buffers() {
        let tree = Tree::new();
        let mut pattern = vec![b'a'; READ_BUFFER_BYTES + 17];
        pattern.push(b'b');
        let value = std::str::from_utf8(&pattern).expect("ASCII pattern");
        let mut contents = vec![b'x'; READ_BUFFER_BYTES / 2];
        contents.extend_from_slice(&pattern);
        tree.file("large.txt", &contents);

        let collisions = analyze_one(&tree.root, value, None);

        assert_eq!(collisions.total, 1);
    }

    #[test]
    fn the_file_size_limit_is_inclusive_and_oversized_files_are_skipped() {
        let exact = Tree::new();
        let mut contents = vec![b'x'; MAX_FILE_BYTES as usize];
        contents[(MAX_FILE_BYTES as usize - 5)..].copy_from_slice(b"value");
        exact.file("exact.txt", &contents);
        assert_eq!(analyze_one(&exact.root, "value", None).total, 1);

        let oversized = Tree::new();
        contents.push(b'x');
        contents[..5].copy_from_slice(b"value");
        oversized.file("oversized.txt", &contents);
        assert!(analyze_one(&oversized.root, "value", None).is_empty());
    }

    #[test]
    fn excluded_directories_are_not_searched() {
        let tree = Tree::new();
        tree.file("node_modules/pkg/index.js", b"value");
        tree.file(".git/objects/x", b"value");
        tree.file("kept.txt", b"value");

        let collisions = analyze_one(&tree.root, "value", None);
        assert_eq!(collisions.total, 1);
        assert_eq!(collisions.files[0].0, "kept.txt");
    }

    #[test]
    #[cfg(unix)]
    fn symlinks_and_special_files_are_skipped() {
        let tree = Tree::new();
        let target = tree.file("target.txt", b"value");
        std::os::unix::fs::symlink(&target, tree.root.join("link.txt")).expect("symlink");
        let fifo = tree.root.join("pipe");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .expect("mkfifo runs")
                .success()
        );

        let collisions = analyze_one(&tree.root, "value", None);
        assert_eq!(collisions.total, 1);
        assert_eq!(collisions.files[0].0, "target.txt");
    }

    #[test]
    fn reports_contain_filenames_and_counts_but_never_values() {
        let canary = Canary::generate("COLLIDING");
        let tree = Tree::new();
        tree.file("config/settings.json", canary.value().as_bytes());

        let collisions = analyze_one(&tree.root, canary.value(), None);
        let description = collisions.describe();
        crate::testing::assert_canary_absent("collision report", description.as_bytes(), &canary);
        assert!(description.contains("config/settings.json"));
        assert!(description.contains('1'));
    }

    #[test]
    fn filenames_are_sanitized_for_the_terminal() {
        let tree = Tree::new();
        tree.file("weird\u{1b}[31mname.txt", b"value");
        let collisions = analyze_one(&tree.root, "value", None);
        assert!(!collisions.files[0].0.contains('\u{1b}'));
        assert!(collisions.files[0].0.contains("\\e[31m"));
    }

    #[test]
    fn several_subjects_share_one_walk() {
        let tree = Tree::new();
        tree.file("a.txt", b"alpha beta beta");
        let results = analyze(
            &tree.root,
            &[
                Subject {
                    value: "alpha",
                    source_files: &[],
                },
                Subject {
                    value: "beta",
                    source_files: &[],
                },
                Subject {
                    value: "gamma",
                    source_files: &[],
                },
            ],
        );
        assert_eq!(results[0].total, 1);
        assert_eq!(results[1].total, 2);
        assert!(results[2].is_empty());
    }

    #[test]
    fn overlapping_patterns_are_counted_independently() {
        let tree = Tree::new();
        tree.file("a.txt", b"aaaa");

        let results = analyze(
            &tree.root,
            &[
                Subject {
                    value: "aa",
                    source_files: &[],
                },
                Subject {
                    value: "aaa",
                    source_files: &[],
                },
            ],
        );

        assert_eq!(results[0].total, 2);
        assert_eq!(results[1].total, 1);
    }
}
