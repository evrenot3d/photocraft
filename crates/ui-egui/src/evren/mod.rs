//! EVREN eklentileri (EVREN add-ons): 360, AI tools, Denoise, PDF import, Line tool. Metinler İngilizce / Türkçe (`dil.rs`).
//! EVREN eklentileri. Photocraft'ın kendi dosyalarına yalnız
//! küçük bağlantı satırları eklenir ("EVREN eklentisi" yorumuyla işaretli), böylece resmi
//! güncellemeler alınırken çakışma az olur. Ayrıntı: depo kökündeki EVREN_GELISTIRME_REHBERI.md.

/// Bu forkun GitHub deposu: güncelleme denetimi (lib.rs `UPDATE_APP`) ve Yardım menüsü bağlantıları buraya bakar.
pub const FORK_REPO: &str = "evrenot3d/photocraft";

pub mod ai_ui;
pub mod arac_cubugu;
pub mod cizgi_ui;
pub mod dil;
pub mod gurultu_ui;
pub mod pano_ui;
pub mod pdf_ui;

/// Her karede çizilen EVREN pencereleri (app/frame.rs'ten).
pub fn pencereler(app: &mut crate::PhotocraftApp, ctx: &egui::Context) {
    // Belge komutlarının (hata mesajı, geçmiş adı, katman adı) dili.
    photocraft_engine::evren_ai_cmds::dil_ayarla(dil::tr());
    pano_ui::window(app, ctx);
    ai_ui::pencere(app, ctx);
    gurultu_ui::pencere(app, ctx);
    pdf_ui::pencere(app, ctx);
    cizgi_ui::pencere(app, ctx);
}
