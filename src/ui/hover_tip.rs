//! Hover tips that stay silent under Settings / Keyboard Shortcuts overlays.

use eframe::egui::{self, Order, Response, WidgetText};

fn block_id() -> egui::Id {
    egui::Id::new("mqtt_master_block_hover_tips")
}

/// Publish whether floating overlays should suppress tips from widgets underneath.
pub fn set_block_hover_tips(ctx: &egui::Context, block: bool) {
    ctx.data_mut(|d| d.insert_temp(block_id(), block));
}

/// True while Settings or Keyboard Shortcuts modal owns the pointer.
pub fn is_blocked(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(block_id()).unwrap_or(false))
}

/// Attach `tip` unless a modal/overlay is open and this widget is not on the overlay layer.
pub fn attach(response: Response, tip: impl Into<WidgetText>) -> Response {
    let blocked = is_blocked(&response.ctx);
    // Foreground = Settings Modal / Shortcuts Modal — tips there stay allowed.
    if blocked && response.layer_id.order != Order::Foreground {
        response
    } else {
        response.on_hover_text(tip)
    }
}
