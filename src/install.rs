//! Per-user install: copy the executable to the user's program dir and add menu + desktop shortcuts. No admin rights.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

const NAME: &str = "Transcription Tool";

fn install_dir() -> Option<PathBuf> {
    let base = directories::BaseDirs::new()?;
    #[cfg(windows)]
    return Some(base.data_local_dir().join("Programs").join(NAME));
    #[cfg(not(windows))]
    return Some(base.data_local_dir().join("transcription-tool"));
}

pub fn installed_exe() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "transcription-tool.exe" } else { "transcription-tool" };
    install_dir().map(|d| d.join(exe))
}

pub fn is_installed_copy() -> bool {
    let (Ok(cur), Some(inst)) = (std::env::current_exe(), installed_exe()) else { return false };
    match (cur.canonicalize(), inst.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn shortcuts() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let desktop = directories::UserDirs::new().and_then(|u| u.desktop_dir().map(Path::to_path_buf));
    #[cfg(windows)]
    {
        if let Some(base) = directories::BaseDirs::new() {
            out.push(base.config_dir().join(r"Microsoft\Windows\Start Menu\Programs").join(format!("{NAME}.lnk")));
        }
        if let Some(d) = desktop {
            out.push(d.join(format!("{NAME}.lnk")));
        }
    }
    #[cfg(not(windows))]
    {
        if let Some(base) = directories::BaseDirs::new() {
            out.push(base.data_local_dir().join("applications/transcription-tool.desktop"));
        }
        if let Some(d) = desktop {
            out.push(d.join("transcription-tool.desktop"));
        }
    }
    out
}

/// Copy self into place, create shortcuts, and start the installed copy.
pub fn install() -> Result<()> {
    let target = installed_exe().context("cannot find the user's program folder")?;
    let current = std::env::current_exe()?;
    std::fs::create_dir_all(target.parent().unwrap())?;
    std::fs::copy(&current, &target)
        .context("Could not copy the program. Close any other open Transcription Tool window and try again.")?;
    for link in shortcuts() {
        if let Some(dir) = link.parent() {
            if dir.is_dir() || std::fs::create_dir_all(dir).is_ok() {
                create_shortcut(&target, &link)?;
            }
        }
    }
    std::process::Command::new(&target).spawn()?;
    Ok(())
}

/// Remove shortcuts, the installed program, downloaded models and settings.
pub fn uninstall() -> Result<()> {
    for link in shortcuts() {
        let _ = std::fs::remove_file(link);
    }
    if let Some(p) = directories::ProjectDirs::from("", "", NAME) {
        let _ = std::fs::remove_dir_all(p.data_local_dir());
        let _ = std::fs::remove_dir_all(p.config_dir());
    }
    let dir = install_dir().context("no install dir")?;
    #[cfg(windows)]
    {
        // A running .exe cannot delete itself on Windows; let cmd remove it after we exit.
        use std::os::windows::process::CommandExt;
        std::process::Command::new("cmd")
            .args(["/C", "timeout /T 3 /NOBREAK >NUL & rmdir /S /Q"])
            .arg(&dir)
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .spawn()?;
    }
    #[cfg(not(windows))]
    std::fs::remove_dir_all(dir)?;
    Ok(())
}

#[cfg(windows)]
fn create_shortcut(target: &Path, link: &Path) -> Result<()> {
    let sl = mslnk::ShellLink::new(target).map_err(|e| anyhow::anyhow!("{e}"))?;
    sl.create_lnk(link).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}

#[cfg(not(windows))]
fn create_shortcut(target: &Path, link: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName={NAME}\nComment=Transcribe audio files\n\
         Exec=\"{}\" %f\nIcon=audio-input-microphone\nTerminal=false\nCategories=AudioVideo;Audio;Utility;\n\
         MimeType=audio/mpeg;audio/mp4;audio/x-m4a;audio/wav;audio/x-wav;audio/flac;audio/ogg;audio/aac;audio/webm;video/mp4;\n",
        target.display()
    );
    std::fs::write(link, entry)?;
    std::fs::set_permissions(link, std::fs::Permissions::from_mode(0o755))?;
    // GNOME only launches desktop-folder launchers marked trusted; best effort.
    let _ = std::process::Command::new("gio").args(["set"]).arg(link).args(["metadata::trusted", "true"]).output();
    Ok(())
}
