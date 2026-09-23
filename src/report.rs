//! A self-contained HTML page with the eval JSON embedded.

use anyhow::{Result, bail};

const TEMPLATE: &str = include_str!("report.html");
const SLOT: &str = "/*__DATA__*/null";

pub fn render_html(json: &str) -> Result<String> {
    if !TEMPLATE.contains(SLOT) {
        bail!("report template is missing its data slot");
    }
    // `<` only occurs inside JSON strings, so escaping it keeps `</script>` and `<!--` inert.
    let safe = json.replace('<', "\\u003c");
    Ok(TEMPLATE.replacen(SLOT, &safe, 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeds_json_and_escapes_angle_brackets() {
        let html = render_html(r#"{"x":"</script><!--<b>"}"#).unwrap();
        assert!(html.contains(r#"{"x":"\u003c/script>\u003c!--\u003cb>"}"#));
        assert!(!html.contains(SLOT));
    }
}
