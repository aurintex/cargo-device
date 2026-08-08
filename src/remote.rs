//! Helpers for building command fragments that are parsed by the **remote** shell.
//!
//! Shared by `run` (the `cd … && exec ./binary` command) and `deploy` (the `mkdir -p`
//! that prepares the deploy path). Everything here produces POSIX-shell syntax — the
//! device runs Linux even when the host does not.

/// Quote a string so a POSIX shell reads it as a single literal word.
///
/// Plain paths and flags are returned unchanged; anything else is single-quoted with
/// embedded single quotes escaped. A `~` is *not* special-cased here — it is quoted like
/// any other character, so use [`remote_path_token`] for paths that should expand
/// against the remote user's home.
pub fn shell_escape(s: &str) -> String {
    if s.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"-_./:".contains(&b))
    {
        s.to_owned()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// Format a path that lives on the **remote** host.
///
/// A leading `~/` becomes `"$HOME"/…` so the remote shell expands it against the device
/// user's home (e.g. `/home/radxa`) rather than the host user's. Everything else is
/// escaped literally via [`shell_escape`].
pub fn remote_path_token(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        format!("\"$HOME\"/{}", shell_escape(rest))
    } else if path == "~" {
        "\"$HOME\"".to_owned()
    } else {
        shell_escape(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_escape_plain_paths_unchanged() {
        assert_eq!(shell_escape("/opt/myapp"), "/opt/myapp");
        assert_eq!(shell_escape("myapp-v2.0"), "myapp-v2.0");
    }

    #[test]
    fn shell_escape_spaces_are_quoted() {
        assert_eq!(shell_escape("my app"), "'my app'");
    }

    #[test]
    fn shell_escape_single_quotes_escaped() {
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
    }

    #[test]
    fn shell_escape_tilde_is_quoted_not_expanded() {
        // shell_escape is the literal-quoting helper; `~` handling belongs to
        // remote_path_token, which callers use for remote paths.
        assert_eq!(shell_escape("~/myapp"), "'~/myapp'");
    }

    #[test]
    fn remote_path_token_expands_leading_tilde_on_the_device() {
        assert_eq!(remote_path_token("~/myapp"), "\"$HOME\"/myapp");
    }

    #[test]
    fn remote_path_token_bare_tilde_is_home() {
        assert_eq!(remote_path_token("~"), "\"$HOME\"");
    }

    #[test]
    fn remote_path_token_absolute_path_unchanged() {
        assert_eq!(remote_path_token("/home/radxa/app"), "/home/radxa/app");
    }

    #[test]
    fn remote_path_token_escapes_spaces_after_tilde() {
        assert_eq!(remote_path_token("~/my app"), "\"$HOME\"/'my app'");
    }
}
