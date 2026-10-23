use std::{collections::VecDeque, path::{Path, PathBuf}, sync::Mutex};

pub struct OpenPaths(Mutex<VecDeque<PathBuf>>);

impl OpenPaths {
    pub fn new(paths: impl IntoIterator<Item = PathBuf>) -> Self {
        Self(Mutex::new(paths.into_iter().collect()))
    }

    pub fn push(&self, paths: impl IntoIterator<Item = PathBuf>) {
        self.0.lock().unwrap().extend(paths);
    }

    pub fn take(&self) -> Vec<String> {
        self.0.lock().unwrap().drain(..).map(|path| path.to_string_lossy().into_owned()).collect()
    }
}

pub fn extract_paths(args: &[String], cwd: &Path) -> Vec<PathBuf> {
    if args.first().is_some_and(|arg| matches!(arg.as_str(), "--mcp" | "--hook" | "--allow")) {
        return Vec::new();
    }
    args.iter().filter(|arg| !arg.starts_with('-')).map(|arg| {
        let path = PathBuf::from(arg);
        if path.is_absolute() { path } else { cwd.join(path) }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_absolute_and_relative_paths_and_ignores_flags() {
        let paths = extract_paths(&[
            "--new-window".into(), "src/main.rs".into(), "C:\\work\\file.txt".into(),
        ], Path::new("C:\\work"));
        assert_eq!(paths, [PathBuf::from("C:\\work\\src\\main.rs"), PathBuf::from("C:\\work\\file.txt")]);
    }

    #[test]
    fn ignores_cli_modes_and_queues_until_taken() {
        for mode in ["--mcp", "--hook", "--allow"] {
            assert!(extract_paths(&[mode.into(), "folder".into()], Path::new("C:\\work")).is_empty());
        }
        let queue = OpenPaths::new(extract_paths(&["file.txt".into()], Path::new("C:\\work")));
        assert_eq!(queue.take(), ["C:\\work\\file.txt"]);
        assert!(queue.take().is_empty());
        queue.push([PathBuf::from("C:\\work\\next.txt")]);
        assert_eq!(queue.take(), ["C:\\work\\next.txt"]);
    }
}
