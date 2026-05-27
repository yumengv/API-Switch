use crate::error::AppError;
use std::path::{Path, PathBuf};

const APP_BINARY_NAMES: &[&str] = &["api-switch", "api-switch.exe"];

pub(crate) fn resolve_data_dir() -> Result<PathBuf, AppError> {
    resolve_data_dir_from(std::env::args_os().map(PathBuf::from), std::env::current_exe)
}

pub(crate) fn database_path() -> Result<PathBuf, AppError> {
    Ok(resolve_data_dir()?.join("api-switch.db"))
}

fn resolve_data_dir_from<I, F>(args: I, current_exe: F) -> Result<PathBuf, AppError>
where
    I: IntoIterator<Item = PathBuf>,
    F: FnOnce() -> std::io::Result<PathBuf>,
{
    if let Some(dir) = args
        .into_iter()
        .find_map(|arg| app_binary_parent_from_arg(&arg))
    {
        return Ok(dir);
    }

    let exe = current_exe()
        .map_err(|e| AppError::Database(format!("Failed to get exe path: {e}")))?;
    exe.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| AppError::Database("Failed to get executable parent directory".to_string()))
}

fn app_binary_parent_from_arg(arg: &Path) -> Option<PathBuf> {
    let file_name = arg.file_name()?.to_str()?;
    if !APP_BINARY_NAMES.contains(&file_name) {
        return None;
    }

    let parent = arg.parent()?;
    if parent.as_os_str().is_empty() {
        return None;
    }

    Some(parent.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loader_invocation_uses_api_switch_argument_parent() {
        let dir = resolve_data_dir_from(
            [
                PathBuf::from("/data/data/com.termux/files/home/as-libs/combined/ld-linux-aarch64.so.1"),
                PathBuf::from("--library-path"),
                PathBuf::from("/data/data/com.termux/files/home/as-libs/combined"),
                PathBuf::from("/data/data/com.termux/files/home/work/as/api-switch"),
                PathBuf::from("--nodisktop"),
            ],
            || Ok(PathBuf::from("/data/data/com.termux/files/home/as-libs/combined/ld-linux-aarch64.so.1")),
        )
        .unwrap();

        assert_eq!(
            dir,
            PathBuf::from("/data/data/com.termux/files/home/work/as")
        );
    }

    #[test]
    fn direct_invocation_falls_back_to_current_exe_parent() {
        let dir = resolve_data_dir_from(
            [PathBuf::from("api-switch"), PathBuf::from("--nodisktop")],
            || Ok(PathBuf::from("/opt/api-switch/api-switch")),
        )
        .unwrap();

        assert_eq!(dir, PathBuf::from("/opt/api-switch"));
    }
}
