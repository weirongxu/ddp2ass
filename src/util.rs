use std::path::PathBuf;

use promkit::crossterm::style::Stylize;

pub fn display_filename(path: &PathBuf) -> String {
    match path.file_name() {
        Some(name) => name.to_string_lossy().underlined().to_string(),
        None => path.display().to_string(),
    }
}
