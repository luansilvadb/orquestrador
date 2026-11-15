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
    args.iter().filter(|arg| !arg.starts_with('-')).filter_map(|arg| {
        // `"%V"` na raiz de um drive vira `"D:\"`, e o Windows lê `\"` como aspa literal: o argumento chega como `D:"`.
        let arg = arg.trim_end_matches('"');
        if arg.is_empty() {
            return None;
        }
        let path = if arg.ends_with(':') { PathBuf::from(format!("{arg}\\")) } else { PathBuf::from(arg) };
        Some(if path.is_absolute() { path } else { cwd.join(path) })
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
    fn restores_drive_root_mangled_by_trailing_backslash_quote() {
        let cwd = Path::new("C:\\work");
        assert_eq!(extract_paths(&["D:\"".into()], cwd), [PathBuf::from("D:\\")]);
        assert_eq!(extract_paths(&["D:\\work\\a b".into()], cwd), [PathBuf::from("D:\\work\\a b")]);
        assert_eq!(extract_paths(&["D:\\".into()], cwd), [PathBuf::from("D:\\")]);
    }

    #[test]
    fn queues_until_taken() {
        let queue = OpenPaths::new(extract_paths(&["file.txt".into()], Path::new("C:\\work")));
        assert_eq!(queue.take(), ["C:\\work\\file.txt"]);
        assert!(queue.take().is_empty());
        queue.push([PathBuf::from("C:\\work\\next.txt")]);
        assert_eq!(queue.take(), ["C:\\work\\next.txt"]);
    }
}
