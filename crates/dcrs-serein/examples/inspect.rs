//! Prints what the scope resolver sees, for diagnosing a theme that converts oddly.
//!
//! ```sh
//! cargo run -p dcrs-serein --example inspect -q -- theme.css
//! ```

use std::collections::BTreeMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: inspect <theme.css>")?;
    let source = std::fs::read_to_string(&path)?;
    let sheet = dcrs_theme::Stylesheet::parse(&source)?;

    println!("rules: {}", sheet.rules().len());
    for rule in sheet.rules() {
        println!(
            "  selectors={:?} scopes={:?} decls={}",
            rule.selectors,
            rule.scopes(),
            rule.declarations.len()
        );
    }

    for kind in [dcrs_theme::ThemeKind::Dark, dcrs_theme::ThemeKind::Light] {
        let vars = dcrs_serein::package::scope_vars(&sheet, kind);
        println!("\n{kind:?} ({} vars)", vars.len());
        let rendered: BTreeMap<&str, &str> =
            vars.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        println!("{rendered:#?}");
    }
    Ok(())
}
