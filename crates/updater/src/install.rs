//! Put a verified package in place of the running copy.
//!
//! - **Windows**: the per-user NSIS installer runs silently into the current folder once the
//!   application has exited (`/UPDATE` makes it wait for `weave-office.exe` to be released),
//!   then starts the new version (`/RELAUNCH`). No administrator rights are involved.
//! - **macOS**: the new `.app` is copied out of the DMG beside the current bundle while the
//!   application runs, checked (bundle identifier, version, code signature), and swapped in
//!   with two renames on restart. A small shell waits for this process to exit, removes the
//!   old bundle and opens the new one.
use crate::release::Version;
use std::path::{Path, PathBuf};

/// Where this copy is installed, when it can replace itself.
#[derive(Clone, Debug)]
pub struct Target {
    /// Windows: the installation folder. macOS: the `.app` bundle.
    pub path: PathBuf,
}

/// A package that has been verified and is ready to be applied.
#[derive(Debug)]
pub struct Prepared {
    #[allow(dead_code)]
    target: Target,
    #[allow(dead_code)]
    package: PathBuf,
    /// macOS: the folder beside the bundle holding the new `.app` (and later the old one).
    #[allow(dead_code)]
    staging: Option<PathBuf>,
}

/// Find out whether this copy can be updated in place, and where.
pub fn target() -> Result<Target, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    // Resolve a symlinked bundle on macOS. Not on Windows: canonical paths there are `\\?\`
    // paths, which the installer's `/D=` does not accept.
    #[cfg(target_os = "macos")]
    let executable = executable.canonicalize().unwrap_or(executable);
    platform::target(&executable)
}

/// Do every step that can fail before the person is asked to restart.
pub fn prepare(target: &Target, package: &Path, version: &Version) -> Result<Prepared, String> {
    platform::prepare(target, package, version)
}

/// Start replacing the application. The caller exits right after this returns `Ok`.
pub fn apply(prepared: Prepared, relaunch: bool) -> Result<(), String> {
    platform::apply(prepared, relaunch)
}

/// The arguments the NSIS installer receives. `/D=` must be last and unquoted.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn installer_arguments(folder: &Path, relaunch: bool) -> (Vec<&'static str>, String) {
    let mut flags = vec!["/S", "/UPDATE"];
    if relaunch {
        flags.push("/RELAUNCH");
    }
    (flags, format!("/D={}", folder.display()))
}

/// Waits for the application to exit, then removes the staged old bundle and opens the new one.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) const RELAUNCH_SCRIPT: &str = r#"pid="$1"; app="$2"; staging="$3"; relaunch="$4"
while kill -0 "$pid" 2>/dev/null; do sleep 0.2; done
rm -rf "$staging"
if [ "$relaunch" = 1 ]; then open "$app"; fi"#;

#[cfg(windows)]
mod platform {
    use super::*;
    use std::os::windows::process::CommandExt;

    pub fn target(executable: &Path) -> Result<Target, String> {
        let folder = executable
            .parent()
            .ok_or_else(|| "no installation folder".to_owned())?;
        // Only a copy the installer put there (portable and development builds have none).
        if !folder.join("Uninstall.exe").is_file() {
            return Err("this copy was not installed by the Weave Office installer".into());
        }
        Ok(Target {
            path: folder.to_path_buf(),
        })
    }

    pub fn prepare(target: &Target, package: &Path, _: &Version) -> Result<Prepared, String> {
        Ok(Prepared {
            target: target.clone(),
            package: package.to_path_buf(),
            staging: None,
        })
    }

    pub fn apply(prepared: Prepared, relaunch: bool) -> Result<(), String> {
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        let (flags, folder) = installer_arguments(&prepared.target.path, relaunch);
        std::process::Command::new(&prepared.package)
            .args(flags)
            .raw_arg(folder)
            .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("could not start the installer: {error}"))
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use std::process::{Command, Stdio};

    pub fn target(executable: &Path) -> Result<Target, String> {
        // …/Weave Office.app/Contents/MacOS/weave-office
        let bundle = executable
            .ancestors()
            .nth(3)
            .filter(|bundle| bundle.extension().is_some_and(|e| e == "app"))
            .ok_or_else(|| "this copy is not an application bundle".to_owned())?;
        let text = bundle.to_string_lossy();
        if text.contains("/AppTranslocation/") || text.starts_with("/Volumes/") {
            return Err("move Weave Office to the Applications folder to update it".into());
        }
        let parent = bundle
            .parent()
            .ok_or_else(|| "no parent folder".to_owned())?;
        // A rename needs write access to the folder holding the bundle.
        let probe = tempfile_in(parent).map_err(|_| {
            format!(
                "no permission to replace the application in {}",
                parent.display()
            )
        })?;
        let _ = std::fs::remove_dir(probe);
        Ok(Target {
            path: bundle.to_path_buf(),
        })
    }

    fn tempfile_in(parent: &Path) -> std::io::Result<PathBuf> {
        let probe = parent.join(format!(".weave-office-update-probe-{}", std::process::id()));
        std::fs::create_dir(&probe)?;
        Ok(probe)
    }

    fn run(command: &mut Command) -> Result<String, String> {
        let output = command
            .stdin(Stdio::null())
            .output()
            .map_err(|error| error.to_string())?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
        }
    }

    fn plist_value(bundle: &Path, key: &str) -> Result<String, String> {
        run(Command::new("/usr/libexec/PlistBuddy")
            .arg("-c")
            .arg(format!("Print :{key}"))
            .arg(bundle.join("Contents/Info.plist")))
    }

    pub fn prepare(target: &Target, package: &Path, version: &Version) -> Result<Prepared, String> {
        let bundle = &target.path;
        let parent = bundle
            .parent()
            .ok_or_else(|| "no parent folder".to_owned())?;
        let staging = parent.join(".Weave Office update");
        if staging.exists() {
            std::fs::remove_dir_all(&staging).map_err(|error| error.to_string())?;
        }
        std::fs::create_dir(&staging).map_err(|error| error.to_string())?;
        let result = (|| {
            let mount = package.with_extension("mount");
            let _ = std::fs::remove_dir(&mount);
            std::fs::create_dir_all(&mount).map_err(|error| error.to_string())?;
            run(Command::new("/usr/bin/hdiutil")
                .args([
                    "attach",
                    "-nobrowse",
                    "-noautoopen",
                    "-readonly",
                    "-mountpoint",
                ])
                .arg(&mount)
                .arg(package))
            .map_err(|error| format!("could not open the disk image: {error}"))?;
            let copied = (|| {
                let app = std::fs::read_dir(&mount)
                    .map_err(|error| error.to_string())?
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .find(|path| {
                        path.extension().is_some_and(|e| e == "app")
                            && !path.is_symlink()
                            && path.is_dir()
                    })
                    .ok_or_else(|| "the disk image holds no application".to_owned())?;
                let staged = staging.join(bundle.file_name().unwrap_or(app.as_os_str()));
                run(Command::new("/usr/bin/ditto").arg(&app).arg(&staged))
                    .map_err(|error| format!("could not copy the application: {error}"))?;
                Ok::<_, String>(staged)
            })();
            let _ = run(Command::new("/usr/bin/hdiutil").arg("detach").arg(&mount)).or_else(|_| {
                run(Command::new("/usr/bin/hdiutil")
                    .args(["detach", "-force"])
                    .arg(&mount))
            });
            let _ = std::fs::remove_dir(&mount);
            let staged = copied?;
            let identifier = plist_value(bundle, "CFBundleIdentifier")?;
            if plist_value(&staged, "CFBundleIdentifier")? != identifier {
                return Err("the downloaded application is a different program".into());
            }
            if plist_value(&staged, "CFBundleShortVersionString")? != version.to_string() {
                return Err("the downloaded application has an unexpected version".into());
            }
            run(Command::new("/usr/bin/codesign")
                .args(["--verify", "--deep", "--strict"])
                .arg(&staged))
            .map_err(|error| format!("the application's signature is invalid: {error}"))?;
            // Our own download carries no quarantine flag; a copied one must not block launch.
            let _ = run(Command::new("/usr/bin/xattr")
                .args(["-dr", "com.apple.quarantine"])
                .arg(&staged));
            Ok(staged)
        })();
        match result {
            Ok(staged) => Ok(Prepared {
                target: target.clone(),
                package: staged,
                staging: Some(staging),
            }),
            Err(error) => {
                let _ = std::fs::remove_dir_all(&staging);
                Err(error)
            }
        }
    }

    pub fn apply(prepared: Prepared, relaunch: bool) -> Result<(), String> {
        let bundle = &prepared.target.path;
        let staging = prepared
            .staging
            .as_ref()
            .ok_or_else(|| "nothing staged".to_owned())?;
        let old = staging.join("previous.app");
        std::fs::rename(bundle, &old)
            .map_err(|error| format!("could not move the old application: {error}"))?;
        if let Err(error) = std::fs::rename(&prepared.package, bundle) {
            let _ = std::fs::rename(&old, bundle);
            return Err(format!("could not install the new application: {error}"));
        }
        Command::new("/bin/sh")
            .arg("-c")
            .arg(RELAUNCH_SCRIPT)
            .arg("sh")
            .arg(std::process::id().to_string())
            .arg(bundle)
            .arg(staging)
            .arg(if relaunch { "1" } else { "0" })
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod platform {
    use super::*;

    pub fn target(_: &Path) -> Result<Target, String> {
        Err("updates are installed by the system's package manager".into())
    }
    pub fn prepare(_: &Target, _: &Path, _: &Version) -> Result<Prepared, String> {
        Err("unsupported platform".into())
    }
    pub fn apply(_: Prepared, _: bool) -> Result<(), String> {
        Err("unsupported platform".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installer_folder_is_the_last_unquoted_argument() {
        let folder = Path::new(r"C:\Users\Ann Lee\AppData\Local\Programs\Weave Office");
        let (flags, last) = installer_arguments(folder, true);
        assert_eq!(flags, ["/S", "/UPDATE", "/RELAUNCH"]);
        assert_eq!(
            last,
            r"/D=C:\Users\Ann Lee\AppData\Local\Programs\Weave Office"
        );
        assert_eq!(installer_arguments(folder, false).0, ["/S", "/UPDATE"]);
    }

    #[test]
    fn relaunch_script_waits_for_the_process_then_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("staging");
        std::fs::create_dir(&staging).unwrap();
        std::fs::write(staging.join("previous"), b"old").unwrap();
        let mut sleeper = std::process::Command::new("sleep")
            .arg("0.5")
            .spawn()
            .unwrap();
        let started = std::time::Instant::now();
        let status = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(RELAUNCH_SCRIPT)
            .arg("sh")
            .arg(sleeper.id().to_string())
            .arg("/nonexistent.app")
            .arg(&staging)
            .arg("0")
            .spawn()
            .unwrap();
        let _ = sleeper.wait();
        let status = { status }.wait().unwrap();
        assert!(status.success());
        assert!(started.elapsed() >= std::time::Duration::from_millis(400));
        assert!(!staging.exists());
    }
}
