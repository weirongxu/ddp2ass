use std::path::PathBuf;

pub fn display_path(path: &PathBuf) -> String {
    match path.file_name() {
        Some(name) => name.to_string_lossy().to_string(),
        None => path.display().to_string(),
    }
}
