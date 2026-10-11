//! File dialogs through the XDG desktop portal, so they look native on any
//! desktop (and work in sandboxes). Falls back to kdialog, then zenity.

use std::collections::HashMap;
use std::path::PathBuf;

use zbus::blocking::{Connection, MessageIterator, Proxy};
use zbus::zvariant::{OwnedValue, Value};

/// Ask for a JSON file to open (`save` = None) or a place to save one
/// (`save` = Some(suggested name)). Blocks until the dialog closes; None if
/// it was cancelled.
pub fn pick_json(title: &str, save: Option<&str>) -> Option<PathBuf> {
    match portal(title, save) {
        Ok(path) => path,
        Err(_) => fallback(title, save),
    }
}

fn portal(title: &str, save: Option<&str>) -> zbus::Result<Option<PathBuf>> {
    let conn = Connection::session()?;
    let token = format!("slidewire{}", std::process::id());
    // The reply arrives as a signal on a request object whose path we can
    // predict; subscribe first so a fast reply isn't missed.
    let sender = conn.unique_name().map(|n| n.trim_start_matches(':').replace('.', "_")).unwrap_or_default();
    let path = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.portal.Request")?
        .member("Response")?
        .path(path.as_str())?
        .build();
    let mut replies = MessageIterator::for_match_rule(rule, &conn, Some(4))?;

    let proxy = Proxy::new(
        &conn,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.FileChooser",
    )?;
    let filter = ("JSON files", vec![(0u32, "*.json")]);
    let mut options: HashMap<&str, Value> = HashMap::new();
    options.insert("handle_token", token.as_str().into());
    options.insert("filters", vec![filter.clone()].into());
    options.insert("current_filter", filter.into());
    let method = match save {
        Some(name) => {
            options.insert("current_name", name.into());
            "SaveFile"
        }
        None => "OpenFile",
    };
    let _: OwnedValue = proxy.call(method, &("", title, options))?;

    let Some(msg) = replies.next() else { return Ok(None) };
    let (response, results): (u32, HashMap<String, OwnedValue>) = msg?.body().deserialize()?;
    if response != 0 {
        return Ok(None);
    }
    let uris: Vec<String> =
        results.get("uris").and_then(|v| v.try_clone().ok()).and_then(|v| v.try_into().ok()).unwrap_or_default();
    Ok(uris.first().and_then(|u| file_path(u)))
}

/// `file:///home/me/My%20Profile.json` -> `/home/me/My Profile.json`.
fn file_path(uri: &str) -> Option<PathBuf> {
    let raw = uri.strip_prefix("file://")?;
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(b) = u8::from_str_radix(&raw[i + 1..i + 3], 16)
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    use std::os::unix::ffi::OsStringExt;
    Some(PathBuf::from(std::ffi::OsString::from_vec(out)))
}

fn fallback(title: &str, save: Option<&str>) -> Option<PathBuf> {
    let attempts: [(&str, Vec<String>); 2] = [
        (
            "kdialog",
            match save {
                Some(name) => vec!["--getsavefilename".into(), name.into(), "*.json|JSON files (*.json)".into()],
                None => vec!["--getopenfilename".into(), ".".into(), "*.json|JSON files (*.json)".into()],
            },
        ),
        ("zenity", {
            let mut a = vec!["--file-selection".into(), format!("--title={title}"), "--file-filter=*.json".into()];
            if let Some(name) = save {
                a.push("--save".into());
                a.push(format!("--filename={name}"));
            }
            a
        }),
    ];
    for (cmd, args) in attempts {
        if let Ok(out) = std::process::Command::new(cmd).args(&args).output() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return (out.status.success() && !path.is_empty()).then(|| PathBuf::from(path));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn file_uris() {
        assert_eq!(
            super::file_path("file:///home/me/My%20Profile%C3%A9.json").unwrap(),
            std::path::PathBuf::from("/home/me/My Profileé.json")
        );
        assert!(super::file_path("https://example.com/x").is_none());
    }
}
