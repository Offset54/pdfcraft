//! How far a mouse-wheel notch scrolls (#444, #186), and ↓ / ↑ with it (#985).
//!
//! egui scrolls `line_scroll_speed` points per wheel line: 40 on the desktop, 8 on the web.
//! macOS already accelerates line deltas, and browsers there scroll about that far per line,
//! but on Windows and Linux winit reports one line per notch, where browsers scroll about
//! 100 pixels, so 40 felt slow. Wheel input reported in points (most trackpads) keeps its own
//! speed, ⌘/Ctrl-wheel zoom keeps egui's rate, and wheel paging in Single page view still turns
//! one page per notch.

/// The user's choice, relative to the platform's normal speed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScrollSpeed {
    Slow,
    #[default]
    Normal,
    Fast,
    /// A finer choice from the hotbar slider, in percent of Normal.
    Custom(u16),
}

/// egui's own `line_scroll_speed`, which its wheel zoom rate is tuned against.
pub(crate) const EGUI_LINE_POINTS: f32 = if cfg!(target_arch = "wasm32") { 8.0 } else { 40.0 };

/// Points one wheel line scrolls at Normal speed.
const NORMAL_LINE_POINTS: f32 = if cfg!(target_arch = "wasm32") {
    // The browser's line deltas, which egui already scales for the web.
    8.0
} else if cfg!(target_os = "macos") {
    40.0
} else {
    100.0
};

impl ScrollSpeed {
    pub const ALL: [Self; 3] = [Self::Slow, Self::Normal, Self::Fast];
    /// The hotbar slider's range and step, in percent of Normal.
    pub const MIN_PERCENT: u16 = 25;
    pub const MAX_PERCENT: u16 = 400;
    pub const STEP_PERCENT: u16 = 5;

    pub fn label(self) -> &'static str {
        match self {
            Self::Slow => "Slow",
            Self::Normal => "Normal",
            Self::Fast => "Fast",
            Self::Custom(_) => "Custom",
        }
    }

    /// This speed relative to Normal.
    pub fn factor(self) -> f32 {
        match self {
            Self::Slow => 0.5,
            Self::Normal => 1.0,
            Self::Fast => 2.0,
            Self::Custom(percent) => f32::from(percent.clamp(Self::MIN_PERCENT, Self::MAX_PERCENT)) / 100.0,
        }
    }

    /// This speed in percent of Normal.
    pub fn percent(self) -> u16 {
        (self.factor() * 100.0).round() as u16
    }

    /// The speed at `percent` of Normal, clamped to the slider's range and step; the presets'
    /// own percentages give back the presets.
    pub fn from_percent(percent: u16) -> Self {
        let step = Self::STEP_PERCENT;
        let percent = (percent.clamp(Self::MIN_PERCENT, Self::MAX_PERCENT) + step / 2) / step * step;
        Self::ALL.into_iter().find(|s| s.percent() == percent).unwrap_or(Self::Custom(percent))
    }

    /// Points one wheel line scrolls.
    pub fn line_points(self) -> f32 {
        NORMAL_LINE_POINTS * self.factor()
    }

    /// Makes egui scroll this far per wheel line, from this frame's input on (call it before egui
    /// reads `raw`). egui scales line deltas before turning ⌘/Ctrl-wheel motion into zoom, so
    /// those events are scaled back to keep zooming at egui's own rate.
    pub(crate) fn apply(self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.set_line_speed(ctx);
        let points = self.line_points();
        let zoom_scale = EGUI_LINE_POINTS / points;
        for event in &mut raw.events {
            if let egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta, modifiers, .. } = event
                && modifiers.command
            {
                *delta *= zoom_scale;
            }
        }
    }

    /// Makes egui scroll this far per wheel line (from the next frame when called during one).
    pub(crate) fn set_line_speed(self, ctx: &egui::Context) {
        let points = self.line_points();
        if ctx.options(|o| o.input_options.line_scroll_speed) != points {
            ctx.options_mut(|o| o.input_options.line_scroll_speed = points);
        }
    }

    /// A preset's name, or a percentage of Normal such as `150` or `150%`.
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        Self::ALL.into_iter().find(|s| s.label().eq_ignore_ascii_case(value)).or_else(|| {
            let percent: u16 = value.strip_suffix('%').unwrap_or(value).trim().parse().ok()?;
            (Self::MIN_PERCENT..=Self::MAX_PERCENT).contains(&percent).then(|| Self::from_percent(percent))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wheel(unit: egui::MouseWheelUnit, dy: f32, command: bool) -> egui::Event {
        let modifiers = egui::Modifiers { command, ..egui::Modifiers::NONE };
        egui::Event::MouseWheel {
            unit,
            delta: egui::vec2(0.0, dy),
            phase: egui::TouchPhase::Move,
            modifiers,
            source: egui::MouseWheelSource::Unknown,
        }
    }

    #[test]
    fn zoom_keeps_egui_rate_and_scrolling_follows_the_choice() {
        let ctx = egui::Context::default();
        for speed in ScrollSpeed::ALL.into_iter().chain([ScrollSpeed::Custom(25), ScrollSpeed::Custom(175), ScrollSpeed::Custom(400)]) {
            let mut raw = egui::RawInput {
                events: vec![
                    wheel(egui::MouseWheelUnit::Line, 1.0, true),
                    wheel(egui::MouseWheelUnit::Line, 1.0, false),
                    wheel(egui::MouseWheelUnit::Point, 30.0, true),
                ],
                ..Default::default()
            };
            speed.apply(&ctx, &mut raw);
            assert_eq!(ctx.options(|o| o.input_options.line_scroll_speed), speed.line_points());
            let dy: Vec<f32> =
                raw.events.iter().filter_map(|e| if let egui::Event::MouseWheel { delta, .. } = e { Some(delta.y) } else { None }).collect();
            // ⌘/Ctrl lines scaled back so egui's zoom sees EGUI_LINE_POINTS per line; plain lines
            // and point deltas untouched.
            assert!((dy[0] * speed.line_points() - EGUI_LINE_POINTS).abs() < 1e-3, "{speed:?}: {dy:?}");
            assert_eq!((dy[1], dy[2]), (1.0, 30.0));
        }
        assert_eq!(ScrollSpeed::parse("FAST"), Some(ScrollSpeed::Fast));
        assert_eq!(ScrollSpeed::parse("warp"), None);
    }

    #[test]
    fn percentages_snap_to_presets_and_round_trip() {
        assert_eq!(ScrollSpeed::from_percent(100), ScrollSpeed::Normal);
        assert_eq!(ScrollSpeed::from_percent(52), ScrollSpeed::Slow);
        assert_eq!(ScrollSpeed::from_percent(200), ScrollSpeed::Fast);
        assert_eq!(ScrollSpeed::from_percent(137), ScrollSpeed::Custom(135));
        assert_eq!(ScrollSpeed::from_percent(1), ScrollSpeed::Custom(ScrollSpeed::MIN_PERCENT));
        assert_eq!(ScrollSpeed::from_percent(9999), ScrollSpeed::Custom(ScrollSpeed::MAX_PERCENT));
        assert!((ScrollSpeed::Custom(150).factor() - 1.5).abs() < 1e-6);
        assert_eq!(ScrollSpeed::parse("150%"), Some(ScrollSpeed::Custom(150)));
        assert_eq!(ScrollSpeed::parse("50"), Some(ScrollSpeed::Slow));
        assert_eq!(ScrollSpeed::parse("5000"), None);
        for speed in [ScrollSpeed::Normal, ScrollSpeed::Custom(175)] {
            let json = serde_json::to_value(speed).unwrap();
            assert_eq!(serde_json::from_value::<ScrollSpeed>(json).unwrap(), speed);
        }
        assert_eq!(serde_json::to_value(ScrollSpeed::Fast).unwrap(), "fast", "presets keep their saved form");
    }
}
