use super::ports::{CoreResult, FileSource};
use std::fs;
use std::path::{Path, PathBuf};

pub struct LocalFileSource;

impl LocalFileSource {
    fn collect(root: &Path, files: &mut Vec<PathBuf>) -> CoreResult<()> {
        for entry in fs::read_dir(root)? {
            let path = entry?.path();
            if path.is_dir() {
                Self::collect(&path, files)?;
            } else if path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
            {
                files.push(path);
            }
        }
        Ok(())
    }
}

impl FileSource for LocalFileSource {
    fn list_markdown(&self, root: &Path) -> CoreResult<Vec<PathBuf>> {
        let mut files = Vec::new();
        Self::collect(root, &mut files)?;
        files.sort();
        Ok(files)
    }

    fn read(&self, path: &Path) -> CoreResult<String> {
        Ok(fs::read_to_string(path)?)
    }

    fn write(&self, path: &Path, content: &str) -> CoreResult<()> {
        fs::write(path, content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_and_reads_markdown_without_including_other_files() {
        let directory = tempfile::tempdir().expect("temp directory");
        let markdown = directory.path().join("note.md");
        let text = directory.path().join("note.txt");
        fs::write(&markdown, "# Note").expect("write markdown");
        fs::write(text, "ignored").expect("write text");

        let source = LocalFileSource;
        let files = source
            .list_markdown(directory.path())
            .expect("list markdown");

        assert_eq!(files, vec![markdown.clone()]);
        assert_eq!(source.read(&markdown).expect("read markdown"), "# Note");
    }
}
