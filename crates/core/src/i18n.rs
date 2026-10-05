//! Языки интерфейса. Все строки — в `crates/core/locales/<код>.json`; ими пользуются и окно (JS),
//! и программа (уведомления, меню, ошибки). Ключ без перевода берётся из английского.
//!
//! Значение — строка с подстановками `{name}` или объект форм для чисел:
//! `{"one": "{n} файл", "few": "{n} файла", "many": "{n} файлов"}` (en: `one`/`other`).

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::{LazyLock, RwLock};

use serde_json::Value;

/// Код и самоназвание. Порядок — как в списке выбора.
pub const LANGS: &[(&str, &str)] = &[
    ("ru", "Русский"),
    ("en", "English"),
    ("de", "Deutsch"),
    ("es", "Español"),
    ("fr", "Français"),
    ("pt", "Português"),
    ("tr", "Türkçe"),
    ("zh", "中文"),
];

const SOURCES: &[(&str, &str)] = &[
    ("ru", include_str!("../locales/ru.json")),
    ("en", include_str!("../locales/en.json")),
    ("de", include_str!("../locales/de.json")),
    ("es", include_str!("../locales/es.json")),
    ("fr", include_str!("../locales/fr.json")),
    ("pt", include_str!("../locales/pt.json")),
    ("tr", include_str!("../locales/tr.json")),
    ("zh", include_str!("../locales/zh.json")),
];

type Dict = HashMap<String, Value>;

static DICTS: LazyLock<HashMap<&'static str, Dict>> = LazyLock::new(|| {
    SOURCES
        .iter()
        .map(|(code, src)| {
            let dict = serde_json::from_str(src).unwrap_or_else(|e| panic!("locales/{code}.json: {e}"));
            (*code, dict)
        })
        .collect()
});

/// Пока не выбран — язык Windows.
static CURRENT: RwLock<Option<&'static str>> = RwLock::new(None);

/// Выбрать язык: код из списка или пустая строка — как в Windows. Возвращает выбранный код.
pub fn set_lang(code: &str) -> &'static str {
    let lang = LANGS.iter().map(|(c, _)| *c).find(|c| *c == code).unwrap_or_else(system_lang);
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = Some(lang);
    lang
}

pub fn lang() -> &'static str {
    CURRENT.read().unwrap_or_else(|e| e.into_inner()).unwrap_or_else(system_lang)
}

/// Язык интерфейса Windows. Для языков стран СНГ без своего перевода — русский.
pub fn system_lang() -> &'static str {
    #[cfg(windows)]
    let primary = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() } & 0x3ff;
    #[cfg(not(windows))]
    let primary = 0x09;
    match primary {
        0x22 | 0x19 | 0x23 | 0x3f | 0x43 | 0x40 | 0x28 | 0x2b | 0x2c | 0x37 | 0x42 => "ru",
        0x07 => "de",
        0x0a => "es",
        0x0c => "fr",
        0x16 => "pt",
        0x1f => "tr",
        0x04 => "zh",
        _ => "en",
    }
}

fn lookup(key: &str) -> Option<&'static Value> {
    let dicts = &*DICTS;
    [lang(), "en", "ru"].iter().find_map(|l| dicts.get(l).and_then(|d| d.get(key)))
}

fn fill(template: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = template.to_string();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), &value.to_string());
    }
    out
}

/// Строка по ключу. Если ключа нет нигде — сам ключ (так пропуски видно сразу).
pub fn t(key: &str) -> String {
    match lookup(key) {
        Some(Value::String(s)) => s.clone(),
        _ => key.to_string(),
    }
}

pub fn tf(key: &str, args: &[(&str, &dyn Display)]) -> String {
    fill(&t(key), args)
}

/// Форма слова для числа: one / few / many / other — по правилам текущего языка.
pub fn plural_form(lang: &str, n: u64) -> &'static str {
    match lang {
        "ru" => {
            let (a, b) = (n % 10, n % 100);
            if a == 1 && b != 11 {
                "one"
            } else if (2..=4).contains(&a) && !(12..=14).contains(&b) {
                "few"
            } else {
                "many"
            }
        }
        "fr" | "pt" => {
            if n <= 1 {
                "one"
            } else {
                "other"
            }
        }
        "zh" => "other",
        _ => {
            if n == 1 {
                "one"
            } else {
                "other"
            }
        }
    }
}

/// «3 файла». В строке форм `{n}` заменяется числом.
pub fn plural(key: &str, n: u64) -> String {
    let form = plural_form(lang(), n);
    let text = match lookup(key) {
        Some(Value::Object(forms)) => forms
            .get(form)
            .or_else(|| forms.get("other"))
            .or_else(|| forms.get("many"))
            .and_then(Value::as_str)
            .unwrap_or(key)
            .to_string(),
        Some(Value::String(s)) => s.clone(),
        _ => key.to_string(),
    };
    text.replace("{n}", &n.to_string())
}

/// Все строки текущего языка (с английскими на месте пропусков) — для окна.
pub fn strings() -> Value {
    let dicts = &*DICTS;
    let mut out = serde_json::Map::new();
    for l in ["ru", "en", lang()] {
        if let Some(d) = dicts.get(l) {
            for (k, v) in d {
                out.insert(k.clone(), v.clone());
            }
        }
    }
    out.insert("_lang".into(), Value::String(lang().into()));
    out.insert(
        "_langs".into(),
        Value::Array(LANGS.iter().map(|(c, n)| serde_json::json!([c, n])).collect()),
    );
    Value::Object(out)
}

/// Названия программы на всех языках (чтобы находить свои ярлыки после смены языка).
pub fn all_values(key: &str) -> Vec<String> {
    let mut v: Vec<String> = DICTS
        .values()
        .filter_map(|d| d.get(key).and_then(Value::as_str).map(str::to_string))
        .collect();
    v.sort();
    v.dedup();
    v
}

/// `t!("ключ")` или `t!("ключ", name = значение, …)`.
#[macro_export]
macro_rules! t {
    ($key:expr) => {
        $crate::i18n::t($key)
    };
    ($key:expr, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::tf($key, &[$((stringify!($name), &$value as &dyn std::fmt::Display)),+])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales_are_complete() {
        let en = &DICTS["en"];
        for (code, _) in LANGS {
            let d = &DICTS[code];
            let missing: Vec<&String> = en.keys().filter(|k| !d.contains_key(*k)).collect();
            assert!(missing.is_empty(), "{code}: нет перевода для {missing:?}");
            let extra: Vec<&String> = d.keys().filter(|k| !en.contains_key(*k)).collect();
            assert!(extra.is_empty(), "{code}: лишние ключи {extra:?}");
            for (k, v) in d {
                if let (Value::String(a), Some(Value::String(b))) = (v, en.get(k)) {
                    let vars = |s: &str| {
                        let mut v: Vec<String> =
                            s.split('{').skip(1).filter_map(|p| p.split_once('}').map(|x| x.0.to_string())).collect();
                        v.sort();
                        v
                    };
                    assert_eq!(vars(a), vars(b), "{code}.{k}: подстановки не совпадают с английскими");
                }
            }
        }
    }

    #[test]
    fn plural_forms() {
        assert_eq!(plural_form("ru", 1), "one");
        assert_eq!(plural_form("ru", 3), "few");
        assert_eq!(plural_form("ru", 11), "many");
        assert_eq!(plural_form("ru", 22), "few");
        assert_eq!(plural_form("en", 1), "one");
        assert_eq!(plural_form("en", 0), "other");
        assert_eq!(plural_form("fr", 0), "one");
    }
}
