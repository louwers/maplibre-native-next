// The std::filesystem operations the C++ runner uses (render-test/filesystem.hpp
// aliases `mln::filesystem` to `std::filesystem`), on POSIX paths held as
// strings: the runner computes test ids by string offsets into them, as C++
// does.

use std::path::Path;
use std::{fs, io};

/// `path::is_relative()` (POSIX: no leading `/`).
pub fn is_relative(path: &str) -> bool {
    !path.starts_with('/')
}

/// `base / rel` (`path::operator/=` on POSIX): an absolute `rel` replaces
/// `base`; otherwise a separator is added unless `base` is empty or already
/// ends with one.
pub fn join(base: &str, rel: &str) -> String {
    if !is_relative(rel) {
        return rel.to_string();
    }
    let mut out = String::with_capacity(base.len() + 1 + rel.len());
    out.push_str(base);
    if !base.is_empty() && !base.ends_with('/') {
        out.push('/');
    }
    out.push_str(rel);
    out
}

/// `path::filename()`: the part after the last `/` (empty for `"a/"`).
pub fn filename(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

/// `path::remove_filename()`: keeps the trailing separator.
pub fn remove_filename(path: &str) -> String {
    match path.rfind('/') {
        Some(i) => path[..=i].to_string(),
        None => String::new(),
    }
}

/// `path::stem()`.
pub fn stem(path: &str) -> &str {
    let name = filename(path);
    if name == "." || name == ".." {
        return name;
    }
    match name.rfind('.') {
        Some(0) | None => name,
        Some(i) => &name[..i],
    }
}

/// `std::filesystem::exists` (follows symlinks).
pub fn exists(path: &str) -> bool {
    Path::new(path).exists()
}

/// `path::lexically_normal()` ([fs.path.generic] normal form, POSIX).
pub fn lexically_normal(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let absolute = path.starts_with('/');
    let ends_with_separator = path.ends_with('/');
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let mut elems: Vec<&str> = Vec::new();
    // Whether a directory separator follows the last remaining element.
    let mut trailing = false;
    for (i, part) in parts.iter().enumerate() {
        let last = i + 1 == parts.len();
        match *part {
            // A dot filename is removed with the separator after it; a final
            // dot leaves the separator before it ("a/." -> "a/").
            "." => {
                if last {
                    trailing = true;
                }
            }
            ".." => {
                if elems.last().is_some_and(|prev| *prev != "..") {
                    // "a/b/.." -> "a/"
                    elems.pop();
                    if last {
                        trailing = true;
                    }
                } else if absolute {
                    // Dot-dot right after the root directory is removed.
                    if last {
                        trailing = true;
                    }
                } else {
                    elems.push("..");
                    if last {
                        trailing = ends_with_separator;
                    }
                }
            }
            name => {
                elems.push(name);
                if last {
                    trailing = ends_with_separator;
                }
            }
        }
    }
    // If the last filename is dot-dot, remove any trailing separator.
    if elems.last() == Some(&"..") {
        trailing = false;
    }
    let mut out = String::new();
    if absolute {
        out.push('/');
    }
    out.push_str(&elems.join("/"));
    if trailing && !elems.is_empty() {
        out.push('/');
    }
    if out.is_empty() {
        out.push('.');
    }
    out
}

/// The entries of `std::filesystem::directory_iterator(dir)`, as
/// `dir / filename` strings, in `readdir` order (libstdc++ does not sort
/// either).
pub fn directory_entries(dir: &str) -> io::Result<Vec<String>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        out.push(join(dir, &entry.file_name().to_string_lossy()));
    }
    Ok(out)
}

/// The entries of `std::filesystem::recursive_directory_iterator(root)`:
/// pre-order, `readdir` order, descending into directories but not into
/// symlinks to directories (no `follow_directory_symlink`).
pub fn recursive_directory_entries(root: &str) -> io::Result<Vec<String>> {
    fn walk(dir: &str, out: &mut Vec<String>) -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = join(dir, &entry.file_name().to_string_lossy());
            let is_dir = entry.file_type()?.is_dir();
            out.push(path.clone());
            if is_dir {
                walk(&path, out)?;
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(root, &mut out)?;
    Ok(out)
}

/// `directory_entry::is_regular_file()` (follows symlinks, such as
/// .cargo/runfiles/maplibre+).
pub fn is_regular_file(path: &str) -> bool {
    fs::metadata(path).map(|m| m.is_file()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexically_normal_matches_std_filesystem() {
        // Examples from [fs.path.generic] / cppreference, plus the shapes the
        // manifests produce.
        let cases = [
            ("", ""),
            ("foo/./bar/..", "foo/"),
            ("foo/.///bar/../", "foo/"),
            ("a/b/../../..", ".."),
            ("/../a", "/a"),
            ("/..", "/"),
            ("/a/..", "/"),
            ("./", "."),
            (".", "."),
            ("a/..", "."),
            ("a/.", "a/"),
            ("a/./b", "a/b"),
            ("../a/", "../a/"),
            ("../a/..", ".."),
            ("a//b", "a/b"),
            ("//", "/"),
            ("a/b/../", "a/"),
            (
                "tools/render-test/manifests/../../../../maplibre+/metrics/integration",
                "../maplibre+/metrics/integration",
            ),
            ("../maplibre+/metrics/./", "../maplibre+/metrics/"),
            ("/x/y/../z", "/x/z"),
        ];
        for (input, expected) in cases {
            assert_eq!(lexically_normal(input), expected, "{input:?}");
        }
    }

    #[test]
    fn join_and_names() {
        assert_eq!(join("a", "b"), "a/b");
        assert_eq!(join("a/", "b"), "a/b");
        assert_eq!(join("", "b"), "b");
        assert_eq!(join("a", ""), "a/");
        assert_eq!(join("a", "/b"), "/b");
        assert_eq!(filename("a/b/style.json"), "style.json");
        assert_eq!(filename("a/b/"), "");
        assert_eq!(remove_filename("a/b/style.json"), "a/b/");
        assert_eq!(remove_filename("style.json"), "");
        assert_eq!(stem("x/linux-wgpu.json"), "linux-wgpu");
        assert_eq!(stem("x/.hidden"), ".hidden");
        assert!(is_relative("a/b") && !is_relative("/a"));
    }
}
