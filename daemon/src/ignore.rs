use std::path::Path;

use ignore::{Walk, WalkBuilder};

pub fn build_walk(base_dir: &Path) -> Walk {
    let ignored_things = [".git", ".ethersync"];
    // TODO: How to deal with binary files?
    WalkBuilder::new(base_dir)
        .standard_filters(true)
        .hidden(false)
        // Interestingly, the standard filters don't seem to ignore .git.
        .filter_entry(move |dir_entry| {
            let name = dir_entry
                .path()
                .file_name()
                .expect("Failed to get file name from path.")
                .to_str()
                .expect("Failed to convert OsStr to str");
            !ignored_things.contains(&name)
        })
        .build()
}

pub fn is_ignored(base_dir: &Path, absolute_file_path: &Path) -> bool {
    // To use the same logic for which files are ignored, iterate through all files
    // using ignore::Walk, and try to find this file.
    // TODO: Request a better way to do this with the "ignore" crate.
    !build_walk(base_dir)
        .filter_map(Result::ok)
        .filter(|dir_entry| {
            dir_entry
                .file_type()
                .expect("Couldn't get file type of dir entry.")
                .is_file()
        })
        .any(|dir_entry| dir_entry.path() == absolute_file_path)
}
