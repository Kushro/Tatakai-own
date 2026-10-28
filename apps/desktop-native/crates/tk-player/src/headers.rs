//! Request-header routing and mpv string quoting (pure).

/// How `MediaSource.headers` map onto mpv options.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeaderPlan {
    pub referrer: Option<String>,
    pub user_agent: Option<String>,
    /// Remaining headers as `Name: value` lines for `http-header-fields`.
    pub fields: Vec<String>,
}

impl HeaderPlan {
    /// Referer/User-Agent get their dedicated options (mpv would otherwise send both); everything else
    /// goes to `http-header-fields`. Headers containing CR/LF are dropped (header injection).
    pub fn from_headers(headers: &[(String, String)]) -> Self {
        let mut plan = Self::default();
        for (name, value) in headers {
            let (name, value) = (name.trim(), value.trim());
            if name.is_empty() || name.contains([':', '\r', '\n']) || value.contains(['\r', '\n']) {
                continue;
            }
            match name.to_ascii_lowercase().as_str() {
                "referer" | "referrer" => plan.referrer = Some(value.to_owned()),
                "user-agent" => plan.user_agent = Some(value.to_owned()),
                _ => plan.fields.push(format!("{name}: {value}")),
            }
        }
        plan
    }
}

/// Encode a string list for an mpv option set through the property API. Every item uses the
/// `%len%item` form so commas inside header values survive (mpv splits lists on a comma).
pub fn mpv_list(items: &[String]) -> String {
    items.iter().map(|i| format!("%{}%{i}", i.len())).collect::<Vec<_>>().join(",")
}

/// Quote one argument for `mpv_command_string` (double quotes, backslash escapes).
pub fn mpv_quote(arg: &str) -> String {
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    for c in arg.chars() {
        match c {
            '\\' | '"' => {
                out.push('\\');
                out.push(c);
            }
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(n: &str, v: &str) -> (String, String) {
        (n.to_owned(), v.to_owned())
    }

    #[test]
    fn routes_referer_and_user_agent_to_their_options() {
        let plan = HeaderPlan::from_headers(&[
            h("Referer", "https://site.example/"),
            h("user-agent", "UA/1.0"),
            h("Origin", "https://site.example"),
            h("Cookie", "a=b, c=d"),
        ]);
        assert_eq!(plan.referrer.as_deref(), Some("https://site.example/"));
        assert_eq!(plan.user_agent.as_deref(), Some("UA/1.0"));
        assert_eq!(plan.fields, ["Origin: https://site.example", "Cookie: a=b, c=d"]);
    }

    #[test]
    fn drops_injection_attempts() {
        let plan = HeaderPlan::from_headers(&[h("X-A", "ok\r\nEvil: 1"), h("Bad:Name", "v"), h("", "v")]);
        assert!(plan.fields.is_empty());
    }

    #[test]
    fn list_items_are_length_prefixed_so_commas_survive() {
        let s = mpv_list(&["Cookie: a=b, c=d".to_owned(), "X: 1".to_owned()]);
        assert_eq!(s, "%16%Cookie: a=b, c=d,%4%X: 1");
        assert_eq!(mpv_list(&[]), "");
    }

    #[test]
    fn quoting_escapes_quotes_and_backslashes() {
        assert_eq!(mpv_quote(r#"a"b\c"#), r#""a\"b\\c""#);
        assert_eq!(mpv_quote("x\ny"), "\"x\\ny\"");
    }
}
