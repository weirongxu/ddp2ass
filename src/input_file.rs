use std::path::{absolute, PathBuf};

use crate::util::display_filename;

pub struct InputFile {
    pub path: PathBuf,
}

impl InputFile {
    pub fn from(filepath: &PathBuf) -> Self {
        let filepath = absolute(filepath).unwrap_or_else(|_| filepath.clone());
        Self { path: filepath }
    }

    pub fn display_filename(self: &Self) -> String {
        display_filename(&self.path)
    }

    pub fn log(self: &Self, s: &str) -> String {
        format!("{} {}", self.display_filename(), s)
    }
}
