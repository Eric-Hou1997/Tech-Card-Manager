//! CommandLineToArgvW-compatible quoting, shared with host-executed tests. There
//! is no shell; all arguments are passed to the fixed packaged executable.
pub fn quote(argument: &[u16]) -> Option<Vec<u16>> {
    if argument.contains(&0) {
        return None;
    }
    let mut result = vec![b'"' as u16];
    let mut slashes = 0;
    for &unit in argument {
        if unit == b'\\' as u16 {
            slashes += 1;
            continue;
        }
        result.extend(std::iter::repeat_n(
            b'\\' as u16,
            if unit == b'"' as u16 {
                slashes * 2 + 1
            } else {
                slashes
            },
        ));
        result.push(unit);
        slashes = 0;
    }
    result.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    result.push(b'"' as u16);
    Some(result)
}

/// The exact Run value written by 4.1.0. This only extracts a path; it never
/// evaluates a command, expands environment variables or accepts extra flags.
pub fn original_login_executable(command: &[u16]) -> Option<Vec<u16>> {
    original_executable(command, "\" --login-startup")
}

pub fn original_agent_executable(command: &[u16]) -> Option<Vec<u16>> {
    original_executable(command, "\" --agent")
}

fn original_executable(command: &[u16], suffix: &str) -> Option<Vec<u16>> {
    let suffix: Vec<u16> = suffix.encode_utf16().collect();
    let path = command
        .strip_prefix(&[b'"' as u16])?
        .strip_suffix(suffix.as_slice())?;
    if path.is_empty() || path.iter().any(|&unit| unit == 0 || unit == b'"' as u16) {
        return None;
    }
    Some(path.to_vec())
}

/// Keep the original login flag, quoting the whole executable as one argument.
pub fn login_command(executable: &[u16]) -> Option<Vec<u16>> {
    if executable.is_empty() || executable.contains(&(b'"' as u16)) {
        return None;
    }
    let mut result = quote(executable)?;
    result.extend(" --login-startup".encode_utf16());
    Some(result)
}

/// Accept our exact current executable and the two existing login flags only.
/// Unquoted validation entries are recognized only when argv[0] has no spaces.
pub fn owns_login_command(command: &[u16], executable: &[u16]) -> bool {
    let Some(original) = login_command(executable) else {
        return false;
    };
    if command == original {
        return true;
    }
    let mut validation = quote(executable).expect("validated executable");
    validation.extend(" --background".encode_utf16());
    if command == validation {
        return true;
    }
    if executable.iter().any(|c| matches!(*c, 9..=13 | 32)) {
        return false;
    }
    let mut unquoted = executable.to_vec();
    unquoted.extend(" --background".encode_utf16());
    command == unquoted
}

/// Registry strings must be terminated UTF-16 with no embedded NUL. Do not
/// expand REG_EXPAND_SZ or interpret malformed data as an absent startup item.
pub fn registry_string(bytes: &[u8]) -> Option<Vec<u16>> {
    if bytes.len() < 2 || !bytes.len().is_multiple_of(2) {
        return None;
    }
    let mut units: Vec<_> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    if units.pop() != Some(0) || units.contains(&0) {
        return None;
    }
    Some(units)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().collect()
    }
    #[test]
    fn login_registration_preserves_original_command_and_rejects_other_owners() {
        let exe = wide(r"C:\Program Files\电影\Tech-Card-Manager.exe");
        let original = wide(r#""C:\Program Files\电影\Tech-Card-Manager.exe" --login-startup"#);
        assert_eq!(login_command(&exe), Some(original.clone()));
        assert!(owns_login_command(&original, &exe));
        assert!(owns_login_command(
            &wide(r#""C:\Program Files\电影\Tech-Card-Manager.exe" --background"#),
            &exe
        ));
        for command in [
            r"C:\Program Files\电影\Tech-Card-Manager.exe --background",
            r#""C:\old\Tech-Card-Manager.exe" --login-startup"#,
            r#""C:\Program Files\电影\Tech-Card-Manager.exe" --agent"#,
            r#""C:\Program Files\电影\Tech-Card-Manager.exe" --login-startup --extra"#,
            r#""C:\Program Files\电影\Tech-Card-Manager.exe" --background & other.exe"#,
        ] {
            assert!(!owns_login_command(&wide(command), &exe), "{command}");
        }
        assert!(owns_login_command(
            &wide(r"C:\TCM.exe --background"),
            &wide(r"C:\TCM.exe")
        ));
        for path in ["", "bad\0.exe", "bad\".exe"] {
            assert!(login_command(&wide(path)).is_none());
            assert!(!owns_login_command(&[], &wide(path)));
        }
    }
    #[test]
    fn registry_string_requires_complete_terminated_data() {
        let command = login_command(&wide(r"C:\电影\TCM.exe")).unwrap();
        let mut bytes: Vec<_> = command
            .iter()
            .chain([&0])
            .flat_map(|c| c.to_le_bytes())
            .collect();
        assert_eq!(registry_string(&bytes), Some(command));
        bytes.truncate(bytes.len() - 2);
        assert_eq!(registry_string(&bytes), None);
        for bad in [
            vec![],
            vec![0],
            vec![65, 0],
            vec![0, 0, 0, 0],
            vec![65, 0, 0, 0, 66, 0, 0, 0],
        ] {
            assert_eq!(registry_string(&bad), None);
        }
    }
    #[test]
    fn original_login_path_is_read_without_interpreting_a_shell_command() {
        let path = "C:\\程序目录\\Tech-Card-Manager.exe";
        assert_eq!(
            original_login_executable(
                &format!("\"{path}\" --login-startup")
                    .encode_utf16()
                    .collect::<Vec<_>>()
            ),
            Some(path.encode_utf16().collect())
        );
        for command in [
            "C:\\TCM.exe --login-startup",
            "\"C:\\TCM.exe\" --agent",
            "\"C:\\TCM.exe\" --login-startup --other",
            "\"\" --login-startup",
            "\"C:\\bad\0.exe\" --login-startup",
            "\"C:\\TCM.exe\" & other \" --login-startup",
        ] {
            assert!(
                original_login_executable(&command.encode_utf16().collect::<Vec<_>>()).is_none()
            );
        }
        assert_eq!(
            original_agent_executable(&wide(r#""C:\电影目录\IMDbTechManager.exe" --agent"#)),
            Some(wide(r"C:\电影目录\IMDbTechManager.exe"))
        );
        for command in [
            r"C:\Manager.exe --agent",
            r#""C:\Manager.exe" --login-startup"#,
            r#""C:\Manager.exe" --agent --other"#,
            r#""C:\Manager.exe" --agent & other.exe"#,
        ] {
            assert!(original_agent_executable(&wide(command)).is_none());
        }
    }
    #[test]
    fn quotes_empty_unicode_spaces_quotes_and_terminal_backslashes() {
        for (input, expected) in [
            ("", "\"\""),
            ("电影 目录", "\"电影 目录\""),
            ("C:\\media\\", "\"C:\\media\\\\\""),
            ("a\"b", "\"a\\\"b\""),
            ("a\\\"b", "\"a\\\\\\\"b\""),
        ] {
            assert_eq!(
                quote(&input.encode_utf16().collect::<Vec<_>>()).unwrap(),
                expected.encode_utf16().collect::<Vec<_>>()
            );
        }
        assert_eq!(
            quote(&[0xd800, b' ' as u16]),
            Some(vec![34, 0xd800, 32, 34])
        );
        assert_eq!(quote(&[65, 0, 66]), None);
    }
}
