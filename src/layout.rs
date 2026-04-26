use ratatui::prelude::Rect;

use crate::config::{Config, LayoutMode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutRects {
    pub banner: Rect,
    pub input: Rect,
    pub separator: Option<Rect>,
    pub list: Rect,
    pub columns: u16,
}

pub fn compute(area: Rect, config: &Config) -> LayoutRects {
    let pad_h = config.layout.padding_horizontal.min(area.width / 2);
    let pad_v = config.layout.padding_vertical.min(area.height / 2);

    let inner_x = area.x + pad_h;
    let inner_y = area.y + pad_v;
    let inner_width = area.width.saturating_sub(pad_h.saturating_mul(2));
    let inner_height = area.height.saturating_sub(pad_v.saturating_mul(2));
    let inner_end_y = inner_y + inner_height;

    let banner_lines: Vec<&str> = if config.ui.banner.is_empty() {
        Vec::new()
    } else {
        config.ui.banner.lines().collect()
    };
    let banner_height = (banner_lines.len() as u16).min(inner_height);
    let banner_width = banner_lines
        .iter()
        .map(|l| l.chars().count() as u16)
        .max()
        .unwrap_or(0)
        .min(inner_width);

    let (banner_x, banner_w) = if config.layout.center_banner {
        let offset = inner_width.saturating_sub(banner_width) / 2;
        (inner_x + offset, banner_width)
    } else {
        (inner_x, inner_width)
    };
    let banner = Rect::new(banner_x, inner_y, banner_w, banner_height);

    let mut cursor_y = inner_y + banner_height;
    let available = inner_end_y.saturating_sub(cursor_y);
    let input_height = available.min(1);
    let input = Rect::new(inner_x, cursor_y, inner_width, input_height);
    cursor_y += input_height;

    let separator = if config.layout.separator && cursor_y < inner_end_y {
        let sep = Rect::new(inner_x, cursor_y, inner_width, 1);
        cursor_y += 1;
        Some(sep)
    } else {
        None
    };

    let list_height = inner_end_y.saturating_sub(cursor_y);
    let list = Rect::new(inner_x, cursor_y, inner_width, list_height);

    let columns = match config.layout.mode {
        LayoutMode::List => 1,
        LayoutMode::Grid => {
            let min_width = config.layout.min_column_width.max(1);
            (list.width / min_width).max(1)
        }
    };

    LayoutRects {
        banner,
        input,
        separator,
        list,
        columns,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, LayoutConfig, LayoutMode, UiConfig};

    fn config_with_layout(layout: LayoutConfig) -> Config {
        Config {
            layout,
            ..Config::default()
        }
    }

    fn bannerless_config_with_layout(layout: LayoutConfig) -> Config {
        Config {
            ui: UiConfig {
                banner: String::new(),
                ..UiConfig::default()
            },
            layout,
            ..Config::default()
        }
    }

    #[test]
    fn default_config_uses_padded_geometry() {
        let cfg = Config::default();
        let area = Rect::new(0, 0, 80, 24);
        let rects = compute(area, &cfg);

        let banner_lines = cfg.ui.banner.lines().count() as u16;
        assert_eq!(rects.banner.x, 4);
        assert_eq!(rects.banner.y, 2);
        assert_eq!(rects.banner.width, 72);
        assert_eq!(rects.banner.height, banner_lines);

        assert_eq!(rects.input.x, 4);
        assert_eq!(rects.input.y, banner_lines + 2);
        assert_eq!(rects.input.width, 72);
        assert_eq!(rects.input.height, 1);

        assert!(rects.separator.is_none());

        assert_eq!(rects.list.x, 4);
        assert_eq!(rects.list.y, banner_lines + 3);
        assert_eq!(rects.list.width, 72);
        assert_eq!(rects.list.height, 20 - banner_lines - 1);
    }

    #[test]
    fn explicit_zero_padding_preserves_dense_geometry() {
        let cfg = config_with_layout(LayoutConfig {
            padding_horizontal: 0,
            padding_vertical: 0,
            ..LayoutConfig::default()
        });
        let area = Rect::new(0, 0, 80, 24);
        let rects = compute(area, &cfg);

        let banner_lines = cfg.ui.banner.lines().count() as u16;
        assert_eq!(rects.banner, Rect::new(0, 0, 80, banner_lines));
        assert_eq!(rects.input, Rect::new(0, banner_lines, 80, 1));
        assert_eq!(
            rects.list,
            Rect::new(0, banner_lines + 1, 80, 24 - banner_lines - 1)
        );
    }

    #[test]
    fn centred_banner_shifts_banner_x() {
        let cfg = config_with_layout(LayoutConfig {
            center_banner: true,
            ..LayoutConfig::default()
        });
        let area = Rect::new(0, 0, 80, 24);
        let rects = compute(area, &cfg);

        let banner_width = cfg
            .ui
            .banner
            .lines()
            .map(|l| l.chars().count() as u16)
            .max()
            .unwrap_or(0);
        let expected_x = 4 + (72 - banner_width) / 2;
        assert_eq!(rects.banner.x, expected_x);
        assert_eq!(rects.banner.width, banner_width);
    }

    #[test]
    fn padding_shrinks_each_rect_on_all_sides() {
        let cfg = config_with_layout(LayoutConfig {
            padding_horizontal: 4,
            padding_vertical: 2,
            ..LayoutConfig::default()
        });
        let area = Rect::new(0, 0, 80, 24);
        let rects = compute(area, &cfg);

        assert_eq!(rects.banner.x, 4);
        assert_eq!(rects.banner.y, 2);
        assert_eq!(rects.input.x, 4);
        assert_eq!(rects.input.width, 72);
        assert_eq!(rects.list.x, 4);
        assert_eq!(rects.list.width, 72);

        let banner_lines = cfg.ui.banner.lines().count() as u16;
        assert_eq!(
            rects.list.y + rects.list.height,
            area.height - 2,
            "bottom padding preserved"
        );
        assert_eq!(rects.input.y, 2 + banner_lines);
    }

    #[test]
    fn separator_enabled_inserts_rect_between_input_and_list() {
        let cfg = config_with_layout(LayoutConfig {
            separator: true,
            ..LayoutConfig::default()
        });
        let area = Rect::new(0, 0, 80, 24);
        let rects = compute(area, &cfg);

        let sep = rects.separator.expect("separator rect present");
        assert_eq!(sep.x, rects.input.x);
        assert_eq!(sep.width, rects.input.width);
        assert_eq!(sep.y, rects.input.y + rects.input.height);
        assert_eq!(sep.height, 1);
        assert_eq!(rects.list.y, sep.y + 1);
    }

    #[test]
    fn separator_disabled_matches_today_behavior() {
        let cfg = config_with_layout(LayoutConfig::default());
        let area = Rect::new(0, 0, 80, 24);
        let rects = compute(area, &cfg);

        assert!(rects.separator.is_none());
        assert_eq!(rects.list.y, rects.input.y + rects.input.height);
    }

    #[test]
    fn empty_banner_produces_zero_height_banner_rect() {
        let cfg = bannerless_config_with_layout(LayoutConfig::default());
        let area = Rect::new(0, 0, 80, 24);
        let rects = compute(area, &cfg);

        assert_eq!(rects.banner.height, 0);
        assert_eq!(rects.input.y, 2);
    }

    #[test]
    fn list_mode_has_single_column() {
        let cfg = Config::default();
        let area = Rect::new(0, 0, 80, 24);
        let rects = compute(area, &cfg);
        assert_eq!(rects.columns, 1);
    }

    #[test]
    fn grid_mode_column_count_matches_width_divided_by_min_width() {
        let cfg = config_with_layout(LayoutConfig {
            mode: LayoutMode::Grid,
            min_column_width: 20,
            ..LayoutConfig::default()
        });
        let area = Rect::new(0, 0, 80, 24);
        let rects = compute(area, &cfg);
        assert_eq!(rects.list.width, 72);
        assert_eq!(rects.columns, 3);
    }

    #[test]
    fn grid_mode_column_count_at_typical_widths() {
        let cases = [
            (80_u16, 20_u16, 3),
            (120, 20, 5),
            (100, 30, 3),
            (200, 40, 4),
        ];
        for (width, min_width, expected_cols) in cases {
            let cfg = config_with_layout(LayoutConfig {
                mode: LayoutMode::Grid,
                min_column_width: min_width,
                ..LayoutConfig::default()
            });
            let area = Rect::new(0, 0, width, 24);
            let rects = compute(area, &cfg);
            assert_eq!(
                rects.columns, expected_cols,
                "width={} min={} expected {} cols, got {}",
                width, min_width, expected_cols, rects.columns
            );
        }
    }

    #[test]
    fn grid_mode_narrow_terminal_collapses_to_single_column() {
        let cfg = config_with_layout(LayoutConfig {
            mode: LayoutMode::Grid,
            min_column_width: 40,
            ..LayoutConfig::default()
        });
        let area = Rect::new(0, 0, 20, 24);
        let rects = compute(area, &cfg);
        assert_eq!(rects.columns, 1);
    }

    #[test]
    fn grid_mode_narrower_than_min_width_does_not_panic() {
        let cfg = config_with_layout(LayoutConfig {
            mode: LayoutMode::Grid,
            min_column_width: 50,
            ..LayoutConfig::default()
        });
        let area = Rect::new(0, 0, 4, 2);
        let rects = compute(area, &cfg);
        assert_eq!(rects.columns, 1);
    }

    #[test]
    fn list_mode_ignores_min_column_width() {
        let cfg_a = config_with_layout(LayoutConfig {
            mode: LayoutMode::List,
            min_column_width: 20,
            ..LayoutConfig::default()
        });
        let cfg_b = config_with_layout(LayoutConfig {
            mode: LayoutMode::List,
            min_column_width: 200,
            ..LayoutConfig::default()
        });
        let area = Rect::new(0, 0, 80, 24);
        let rects_a = compute(area, &cfg_a);
        let rects_b = compute(area, &cfg_b);
        assert_eq!(rects_a.columns, 1);
        assert_eq!(rects_b.columns, 1);
        assert_eq!(rects_a.list, rects_b.list);
    }

    #[test]
    fn tiny_area_does_not_panic_or_overflow() {
        let cfg = config_with_layout(LayoutConfig {
            padding_horizontal: 10,
            padding_vertical: 10,
            separator: true,
            ..LayoutConfig::default()
        });
        let area = Rect::new(0, 0, 4, 2);
        let rects = compute(area, &cfg);

        assert!(rects.banner.x + rects.banner.width <= area.x + area.width);
        assert!(rects.list.y + rects.list.height <= area.y + area.height);
    }
}
