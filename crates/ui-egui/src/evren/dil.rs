//! EVREN eklentileri: iki dil (English / Türkçe).
//!
//! Eklentilerin metinleri PhotoCraft'ın arayüz diline uyar: Türkçe arayüzde Türkçe, başka her dilde İngilizce.
//! `evren_ai\ayarlar.json` içindeki `"eklenti_dili": "tr" | "en"` bunu yalnız eklentiler için değiştirir.
//! Kodda: `t!("English", "Türkçe")` (iki taraf aynı türde: &str ya da String).

use std::sync::OnceLock;

fn zorla() -> Option<bool> {
    static Z: OnceLock<Option<bool>> = OnceLock::new();
    *Z.get_or_init(|| {
        let p = super::ai_ui::kok()?.join("ayarlar.json");
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(p).ok()?).ok()?;
        match v["eklenti_dili"].as_str()? {
            "tr" => Some(true),
            "en" => Some(false),
            _ => None,
        }
    })
}

/// Eklentiler Türkçe mi gösterilsin.
pub fn tr() -> bool {
    zorla().unwrap_or_else(|| crate::i18n::current().code() == "tr")
}

/// Yapay zekâ motoruna (Python) ve belge komutlarına giden dil kodu.
pub fn kod() -> &'static str {
    if tr() { "tr" } else { "en" }
}

/// `t!("English", "Türkçe")`
#[macro_export]
macro_rules! t {
    ($en:expr, $tr:expr $(,)?) => {
        if $crate::evren::dil::tr() { $tr } else { $en }
    };
}
