//! Admin shell i18n — translate the framework's own UI strings (buttons,
//! navigation, titles, the account page, the composition editor, …) for the
//! active language. This is the *shell* layer; field/value **data** labels
//! live in the ViewSpec and stay subject to the iron rule. Here too the iron
//! rule holds: English is the source and the key, only the shown text changes.
//!
//! Keys are the **English source string** (gettext-style), so a missing
//! translation transparently falls back to readable English — never a blank
//! or a raw key. Built-in defaults ship Swedish as the reference locale;
//! projects extend or override any string via a `rustio.locale.json` at the
//! project root: `{ "sv": { "Add": "Lägg till" }, "ar": { … } }`.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Built-in reference translations, bundled in the binary so the admin is
/// usable in these languages with no project file. `en` is implicit (the key).
fn builtin() -> HashMap<&'static str, &'static [(&'static str, &'static str)]> {
    let sv: &[(&str, &str)] = &[
        // Navigation / shell
        ("Home", "Hem"),
        ("Workspace", "Arbetsyta"),
        ("System", "System"),
        ("Recent actions", "Senaste åtgärder"),
        ("Your account", "Ditt konto"),
        ("Dashboard", "Översikt"),
        ("Signed in as", "Inloggad som"),
        ("Log out", "Logga ut"),
        ("Sign out", "Logga ut"),
        ("Language", "Språk"),
        ("Default", "Standard"),
        ("Project default", "Projektets standard"),
        // List toolbar / table
        ("Search", "Sök"),
        ("Showing", "Visar"),
        ("of", "av"),
        ("record", "post"),
        ("records", "poster"),
        ("No records yet", "Inga poster ännu"),
        ("Set as default", "Ange som standard"),
        ("Edit view", "Redigera vy"),
        ("Table", "Tabell"),
        ("List", "Lista"),
        ("Cards", "Kort"),
        ("Compact", "Kompakt"),
        ("Filter", "Filter"),
        ("All", "Alla"),
        ("Clear", "Rensa"),
        ("Apply", "Tillämpa"),
        ("Yes", "Ja"),
        ("No", "Nej"),
        // Row / record actions
        ("Add", "Lägg till"),
        ("Edit", "Redigera"),
        ("Delete", "Ta bort"),
        ("View", "Visa"),
        ("Back", "Tillbaka"),
        ("Back to list", "Tillbaka till listan"),
        ("Back to admin", "Tillbaka till admin"),
        ("Cancel", "Avbryt"),
        ("Save", "Spara"),
        ("Save view", "Spara vy"),
        ("Create", "Skapa"),
        ("New", "Ny"),
        (
            "Get started by creating your first record.",
            "Kom igång genom att skapa din första post.",
        ),
        // Forms
        ("Required", "Obligatoriskt"),
        ("Optional", "Valfritt"),
        ("Not saved.", "Inte sparat."),
        // Composition editor
        ("Order", "Ordning"),
        ("Field", "Fält"),
        ("Role", "Roll"),
        ("Merge into", "Slå samman med"),
        ("Display label", "Visningsetikett"),
        ("Value labels", "Värdeetiketter"),
        ("Value", "Värde"),
        ("Editing labels in:", "Redigerar etiketter på:"),
        ("View default:", "Vyns standard:"),
        ("Title", "Titel"),
        ("Subtitle", "Underrubrik"),
        ("Badge", "Märke"),
        ("Timestamp", "Tidsstämpel"),
        ("Meta", "Meta"),
        ("Hidden", "Dold"),
        // Account / role page
        ("Change password", "Byt lösenord"),
        ("Role & permissions", "Roll och behörigheter"),
        ("Roles in this workspace", "Roller i arbetsytan"),
        ("Account details", "Kontouppgifter"),
        ("Security", "Säkerhet"),
        ("Email", "E-post"),
        ("User ID", "Användar-ID"),
        ("Member since", "Medlem sedan"),
        ("Active sessions", "Aktiva sessioner"),
        ("Sign out other sessions", "Logga ut andra sessioner"),
        ("You", "Du"),
        ("Administrator", "Administratör"),
        ("Editor", "Redaktör"),
        ("Viewer", "Visare"),
        ("You're signed in as", "Du är inloggad som"),
        // Account page — role blurbs (one per role tier)
        (
            "You can manage every record, user, and list view in this workspace. Schema evolution and the CLI are developer tools, run outside the admin.",
            "Du kan hantera alla poster, användare och listvyer i arbetsytan. Schemaändringar och CLI är utvecklarverktyg som körs utanför admin.",
        ),
        (
            "You manage records and list views across the workspace; framework tables stay read-only.",
            "Du hanterar poster och listvyer i arbetsytan; ramverkets tabeller förblir skrivskyddade.",
        ),
        (
            "You can create and edit records across every model, but not delete them.",
            "Du kan skapa och redigera poster i alla modeller, men inte ta bort dem.",
        ),
        (
            "You have read-only access to every record in this workspace.",
            "Du har skrivskyddad åtkomst till alla poster i arbetsytan.",
        ),
        // Account page — permission rows (label + detail)
        ("View records", "Visa poster"),
        ("Read every model in the workspace", "Läs alla modeller i arbetsytan"),
        ("Create & edit", "Skapa och redigera"),
        (
            "Add and update records across all models",
            "Lägg till och uppdatera poster i alla modeller",
        ),
        ("Delete records", "Ta bort poster"),
        ("Remove records, with confirmation", "Ta bort poster, med bekräftelse"),
        ("Manage users & roles", "Hantera användare och roller"),
        (
            "Create users and assign their roles",
            "Skapa användare och tilldela roller",
        ),
        ("Reshape list views", "Forma om listvyer"),
        (
            "Edit ViewSpec roles, filters, and labels",
            "Redigera ViewSpec-roller, filter och etiketter",
        ),
        ("Evolve schema", "Utveckla schemat"),
        (
            "Add or change fields — a developer / CLI tool",
            "Lägg till eller ändra fält – ett utvecklar-/CLI-verktyg",
        ),
        ("Run migrations & CLI", "Kör migreringar och CLI"),
        (
            "Apply migrations and use the rustio CLI",
            "Tillämpa migreringar och använd rustio-CLI:t",
        ),
        // Account page — role reference descriptions
        (
            "Read-only access to every record in the workspace.",
            "Skrivskyddad åtkomst till alla poster i arbetsytan.",
        ),
        (
            "Create and edit records across all models; cannot delete.",
            "Skapa och redigera poster i alla modeller; kan inte ta bort.",
        ),
        (
            "Manages all records, users, and list views.",
            "Hanterar alla poster, användare och listvyer.",
        ),
        // Pagination
        ("Previous", "Föregående"),
        ("Next", "Nästa"),
    ];
    let mut m = HashMap::new();
    m.insert("sv", sv);
    m
}

/// The merged catalog: built-in defaults plus any project `rustio.locale.json`
/// overrides (the project file wins per key). Loaded once per process.
fn catalog() -> &'static HashMap<String, HashMap<String, String>> {
    static CELL: OnceLock<HashMap<String, HashMap<String, String>>> = OnceLock::new();
    CELL.get_or_init(|| {
        let mut cat: HashMap<String, HashMap<String, String>> = builtin()
            .into_iter()
            .map(|(lang, pairs)| {
                (
                    lang.to_string(),
                    pairs
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect(),
                )
            })
            .collect();
        // Lenient merge: only object sections of string→string are applied,
        // so a `"_comment": "…"` doc key (or any stray value) is ignored
        // instead of rejecting the whole file.
        if let Ok(raw) = std::fs::read_to_string("rustio.locale.json") {
            if let Ok(serde_json::Value::Object(top)) =
                serde_json::from_str::<serde_json::Value>(&raw)
            {
                for (lang, section) in top {
                    if let serde_json::Value::Object(map) = section {
                        let entry = cat.entry(lang).or_default();
                        for (k, v) in map {
                            if let serde_json::Value::String(s) = v {
                                entry.insert(k, s);
                            }
                        }
                    }
                }
            }
        }
        cat
    })
}

/// Every language code with at least one translation in the merged catalog
/// (built-in + project file). Lets the switcher offer every language the
/// project has localised, not just the built-ins.
pub fn catalog_languages() -> Vec<String> {
    let mut v: Vec<String> = catalog().keys().cloned().collect();
    v.sort();
    v
}

/// Best-effort endonym (a language's own name) for switcher labels; falls
/// back to the uppercased code for languages not in this small table.
pub fn endonym(code: &str) -> String {
    match code {
        "en" => "English",
        "sv" => "Svenska",
        "de" => "Deutsch",
        "fr" => "Français",
        "es" => "Español",
        "it" => "Italiano",
        "pt" => "Português",
        "nl" => "Nederlands",
        "da" => "Dansk",
        "no" => "Norsk",
        "fi" => "Suomi",
        "is" => "Íslenska",
        "ar" => "العربية",
        "pl" => "Polski",
        "ru" => "Русский",
        "uk" => "Українська",
        "zh" => "中文",
        "ja" => "日本語",
        "ko" => "한국어",
        "tr" => "Türkçe",
        _ => return code.to_uppercase(),
    }
    .to_string()
}

/// Translate an English source `key` for `lang`. Empty / `"en"` / a missing
/// entry returns the key (the English source) — never blank.
pub fn translate(lang: &str, key: &str) -> String {
    if lang.is_empty() || lang == "en" {
        return key.to_string();
    }
    catalog()
        .get(lang)
        .and_then(|m| m.get(key))
        .cloned()
        .unwrap_or_else(|| key.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_known_strings_and_falls_back_to_english() {
        assert_eq!(translate("sv", "Add"), "Lägg till");
        assert_eq!(translate("sv", "Save"), "Spara");
        // en / empty / unknown lang → the English source key.
        assert_eq!(translate("en", "Add"), "Add");
        assert_eq!(translate("", "Add"), "Add");
        assert_eq!(translate("de", "Add"), "Add");
        // A key with no translation in an otherwise-known language → English.
        assert_eq!(
            translate("sv", "Some Unlisted String"),
            "Some Unlisted String"
        );
    }

    #[test]
    fn catalog_languages_includes_builtin_and_endonyms_resolve() {
        // Swedish ships built-in, so it's always discoverable.
        assert!(catalog_languages().contains(&"sv".to_string()));
        assert_eq!(endonym("sv"), "Svenska");
        assert_eq!(endonym("de"), "Deutsch");
        // Unknown code → its own uppercased form, never blank.
        assert_eq!(endonym("xx"), "XX");
    }
}
