use std::{
    error::Error,
    path::{Path, PathBuf},
};

pub fn collect(root: &Path, extensions: &[&str]) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    Ok(gdview::Project::open(root)?.files(
        extensions,
        gdview::FileOptions {
            case_insensitive_extensions: true,
            exclude_hidden: true,
            respect_gitignore: true,
            respect_gdignore: true,
        },
    )?)
}
