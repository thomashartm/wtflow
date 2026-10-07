//! Clipboard transport shared by terminal frontends. Text is never evaluated by a shell.
use anyhow::{bail, Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::{
    io::Write,
    process::{Command, Stdio},
};

type Helper<'a> = (&'a str, &'a [&'a str]);

pub fn copy_key(key: KeyEvent) -> bool {
    (key.modifiers
        .contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT)
        && matches!(key.code, KeyCode::Char('c' | 'C')))
        || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Insert)
        || (key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Char('y'))
        || (key.modifiers.contains(KeyModifiers::SUPER) && key.code == KeyCode::Char('c'))
}

pub fn paste_key(key: KeyEvent) -> bool {
    (key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('v' | 'V')))
        || (key.modifiers.contains(KeyModifiers::SHIFT) && key.code == KeyCode::Insert)
        || (key.modifiers.contains(KeyModifiers::SUPER) && key.code == KeyCode::Char('v'))
}

pub fn copy(text: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let helpers: &[Helper<'_>] = &[("pbcopy", &[])];
    #[cfg(target_os = "windows")]
    let helpers: &[Helper<'_>] = &[(
        "powershell.exe",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$input | Set-Clipboard",
        ],
    )];
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let helpers: &[Helper<'_>] = &[
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--input"]),
    ];
    write_with(helpers, text)
}

fn write_with(helpers: &[Helper<'_>], text: &str) -> Result<()> {
    for (program, args) in helpers {
        let Ok(mut child) = Command::new(program)
            .args(*args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        let written = child
            .stdin
            .take()
            .context("clipboard stdin")
            .and_then(|mut stdin| Ok(stdin.write_all(text.as_bytes())?));
        // Always close stdin and reap the helper, including failed writes.
        let status = child.wait();
        if written.is_ok() && status.is_ok_and(|status| status.success()) {
            return Ok(());
        }
    }
    bail!("Clipboard unavailable: no working system clipboard helper")
}

pub fn read() -> Result<String> {
    #[cfg(target_os = "macos")]
    let helpers: &[Helper<'_>] = &[("pbpaste", &[])];
    #[cfg(target_os = "windows")]
    let helpers: &[Helper<'_>] = &[("powershell.exe", &["-NoProfile", "-NonInteractive", "-Command", "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; [Console]::Write((Get-Clipboard -Raw))"])];
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let helpers: &[Helper<'_>] = &[
        ("wl-paste", &["--no-newline"]),
        ("xclip", &["-selection", "clipboard", "-o"]),
        ("xsel", &["--clipboard", "--output"]),
    ];
    read_with(helpers)
}

fn read_with(helpers: &[Helper<'_>]) -> Result<String> {
    for (program, args) in helpers {
        if let Ok(output) = Command::new(program)
            .args(*args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
        {
            if output.status.success() {
                return String::from_utf8(output.stdout).context("Clipboard is not UTF-8 text");
            }
        }
    }
    bail!("Clipboard unavailable: use your terminal's Paste command")
}

/// Search stays on one line; field values retain multiline YAML and labels.
pub fn paste_text(text: &str, multiline: bool) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .filter_map(|c| match c {
            '\n' | '\t' if !multiline => Some(' '),
            '\n' | '\t' => Some(c),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paste_retains_unicode_and_line_boundaries_without_control_keys() {
        let text = "café 🐈\r\n  next\tvalue\rfinal\x1b\0\x08";
        assert_eq!(paste_text(text, true), "café 🐈\n  next\tvalue\nfinal");
        assert_eq!(paste_text(text, false), "café 🐈   next value final");
    }
    #[test]
    fn shortcuts_keep_plain_typing_and_cancellation_separate() {
        assert!(!copy_key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL
        )));
        assert!(copy_key(KeyEvent::new(
            KeyCode::Char('C'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT
        )));
        assert!(copy_key(KeyEvent::new(
            KeyCode::Char('y'),
            KeyModifiers::ALT
        )));
        assert!(paste_key(KeyEvent::new(
            KeyCode::Char('v'),
            KeyModifiers::CONTROL
        )));
        assert!(paste_key(KeyEvent::new(
            KeyCode::Insert,
            KeyModifiers::SHIFT
        )));
        assert!(!paste_key(KeyEvent::new(
            KeyCode::Char('v'),
            KeyModifiers::NONE
        )));
    }
    #[cfg(unix)]
    #[test]
    fn helpers_fall_back_after_failure_and_preserve_literal_text() {
        use std::{fs, os::unix::fs::PermissionsExt};
        let temp = tempfile::tempdir().unwrap();
        let helper = temp.path().join("clipboard");
        let data = temp.path().join("text");
        fs::write(&helper, "#!/bin/sh\ncase \"$1\" in\nwrite) cat > \"$2\" ;;\nread) cat \"$2\" ;;\n*) exit 1 ;;\nesac\n").unwrap();
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
        let helper = helper.to_str().unwrap();
        let data = data.to_str().unwrap();
        let text = "café 🐈\n$(do-not-execute) `literal`\n";
        write_with(&[(helper, &["fail"]), (helper, &["write", data])], text).unwrap();
        assert_eq!(
            read_with(&[(helper, &["fail"]), (helper, &["read", data])]).unwrap(),
            text
        );
        assert!(write_with(&[(helper, &["fail"])], text).is_err());
        assert!(read_with(&[(helper, &["fail"])]).is_err());
    }
}
