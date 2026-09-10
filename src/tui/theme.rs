use ratatui::style::{Color, Modifier, Style};

use std::collections::BTreeMap;

use crate::settings::{ThemePalette, TuiSettings};

#[derive(Clone, Copy)]
pub(super) struct Theme {
    pub(super) background: Style,
    pub(super) text: Style,
    pub(super) muted: Style,
    pub(super) border: Style,
    pub(super) focus: Style,
    pub(super) selected: Style,
    pub(super) important: Style,
    pub(super) test: Style,
    pub(super) usage: Style,
    pub(super) definition: Style,
    pub(super) addition_line: Style,
    pub(super) deletion_line: Style,
    pub(super) warning: Style,
    pub(super) error: Style,
    pub(super) syntax: [Style; 8],
}

impl Theme {
    pub(super) fn resolve(
        configured: &str,
        custom: &BTreeMap<String, ThemePalette>,
        no_color: bool,
    ) -> Self {
        if no_color || configured == "terminal" {
            return Self::monochrome();
        }
        if let Some(palette) = custom.get(configured) {
            return Self::from_palette(palette);
        }
        let truecolor = std::env::var("COLORTERM")
            .is_ok_and(|value| value.contains("truecolor") || value.contains("24bit"));
        let palette =
            builtin_palette(configured).unwrap_or_else(|| builtin_palette("night-owl").unwrap());
        if truecolor {
            Self::from_palette(&palette)
        } else {
            Self::indexed(configured)
        }
    }

    fn monochrome() -> Self {
        let plain = Style::default();
        Self {
            background: plain,
            text: plain,
            muted: plain.add_modifier(Modifier::DIM),
            border: plain,
            focus: plain.add_modifier(Modifier::BOLD),
            selected: plain.add_modifier(Modifier::REVERSED | Modifier::BOLD),
            important: plain.add_modifier(Modifier::BOLD),
            test: plain,
            usage: plain,
            definition: plain,
            addition_line: plain.add_modifier(Modifier::BOLD),
            deletion_line: plain.add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            warning: plain.add_modifier(Modifier::BOLD),
            error: plain.add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            syntax: [plain; 8],
        }
    }

    fn from_palette(palette: &ThemePalette) -> Self {
        let background = parse_color(&palette.background);
        let foreground = parse_color(&palette.foreground);
        let addition = parse_color(&palette.addition);
        let deletion = parse_color(&palette.deletion);
        Self {
            background: Style::default().bg(background).fg(foreground),
            text: Style::default().fg(foreground).bg(background),
            muted: Style::default()
                .fg(parse_color(&palette.muted))
                .bg(background),
            border: Style::default()
                .fg(parse_color(&palette.border))
                .bg(background),
            focus: Style::default()
                .fg(parse_color(&palette.focus))
                .bg(background),
            selected: Style::default()
                .fg(background)
                .bg(parse_color(&palette.selection))
                .add_modifier(Modifier::BOLD),
            important: Style::default()
                .fg(parse_color(&palette.important))
                .bg(background),
            test: Style::default()
                .fg(parse_color(&palette.test))
                .bg(background),
            usage: Style::default()
                .fg(parse_color(&palette.usage))
                .bg(background),
            definition: Style::default()
                .fg(parse_color(&palette.definition))
                .bg(background),
            addition_line: Style::default()
                .fg(addition)
                .bg(blend(background, addition)),
            deletion_line: Style::default()
                .fg(deletion)
                .bg(blend(background, deletion)),
            warning: Style::default()
                .fg(parse_color(&palette.warning))
                .bg(background),
            error: Style::default()
                .fg(parse_color(&palette.error))
                .bg(background),
            syntax: [
                &palette.syntax.comment,
                &palette.syntax.string,
                &palette.syntax.number,
                &palette.syntax.keyword,
                &palette.syntax.function,
                &palette.syntax.r#type,
                &palette.syntax.property,
                &palette.syntax.variable,
            ]
            .map(|color| Style::default().fg(parse_color(color)).bg(background)),
        }
    }

    fn indexed(_name: &str) -> Self {
        let background = Color::Indexed(17);
        let foreground = Color::Indexed(189);
        Self {
            background: Style::default().bg(background).fg(foreground),
            text: Style::default().fg(foreground).bg(background),
            muted: Style::default().fg(Color::Indexed(66)).bg(background),
            border: Style::default().fg(Color::Indexed(67)).bg(background),
            focus: Style::default().fg(Color::Indexed(116)).bg(background),
            selected: Style::default()
                .fg(background)
                .bg(Color::Indexed(116))
                .add_modifier(Modifier::BOLD),
            important: Style::default().fg(Color::Indexed(215)).bg(background),
            test: Style::default().fg(Color::Indexed(149)).bg(background),
            usage: Style::default().fg(Color::Indexed(116)).bg(background),
            definition: Style::default().fg(Color::Indexed(176)).bg(background),
            addition_line: Style::default()
                .fg(Color::Indexed(149))
                .bg(Color::Indexed(22)),
            deletion_line: Style::default()
                .fg(Color::Indexed(203))
                .bg(Color::Indexed(52)),
            warning: Style::default().fg(Color::Indexed(215)).bg(background),
            error: Style::default().fg(Color::Indexed(203)).bg(background),
            syntax: [
                Color::Indexed(66),
                Color::Indexed(149),
                Color::Indexed(209),
                Color::Indexed(176),
                Color::Indexed(111),
                Color::Indexed(215),
                Color::Indexed(116),
                foreground,
            ]
            .map(|color| Style::default().fg(color).bg(background)),
        }
    }
}

pub(crate) fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        "night-owl" | "tokyo-night" | "catppuccin-mocha" | "terminal"
    )
}

pub(super) fn available(settings: &TuiSettings) -> Vec<String> {
    let mut names = vec![
        "night-owl".into(),
        "tokyo-night".into(),
        "catppuccin-mocha".into(),
        "terminal".into(),
    ];
    names.extend(
        settings
            .themes
            .keys()
            .filter(|name| !is_builtin(name))
            .cloned(),
    );
    names
}

fn builtin_palette(name: &str) -> Option<ThemePalette> {
    let colors = match name {
        "night-owl" => [
            "#011627", "#D6DEEB", "#637777", "#5C7E8D", "#7FDBCA", "#7FDBCA", "#FFCB8B", "#ADDB67",
            "#7FDBCA", "#C792EA", "#ADDB67", "#EF5350", "#FFCB8B", "#EF5350",
        ],
        "tokyo-night" => [
            "#1A1B26", "#C0CAF5", "#565F89", "#3B4261", "#7DCFFF", "#7AA2F7", "#E0AF68", "#9ECE6A",
            "#7DCFFF", "#BB9AF7", "#9ECE6A", "#F7768E", "#E0AF68", "#F7768E",
        ],
        "catppuccin-mocha" => [
            "#1E1E2E", "#CDD6F4", "#6C7086", "#585B70", "#89DCEB", "#89B4FA", "#F9E2AF", "#A6E3A1",
            "#89DCEB", "#CBA6F7", "#A6E3A1", "#F38BA8", "#F9E2AF", "#F38BA8",
        ],
        _ => return None,
    };
    Some(ThemePalette {
        background: colors[0].into(),
        foreground: colors[1].into(),
        muted: colors[2].into(),
        border: colors[3].into(),
        focus: colors[4].into(),
        selection: colors[5].into(),
        important: colors[6].into(),
        test: colors[7].into(),
        usage: colors[8].into(),
        definition: colors[9].into(),
        addition: colors[10].into(),
        deletion: colors[11].into(),
        warning: colors[12].into(),
        error: colors[13].into(),
        syntax: crate::settings::SyntaxPalette::default(),
    })
}

fn parse_color(value: &str) -> Color {
    let red = u8::from_str_radix(&value[1..3], 16).unwrap_or(255);
    let green = u8::from_str_radix(&value[3..5], 16).unwrap_or(255);
    let blue = u8::from_str_radix(&value[5..7], 16).unwrap_or(255);
    Color::Rgb(red, green, blue)
}

fn blend(background: Color, foreground: Color) -> Color {
    let (Color::Rgb(br, bg, bb), Color::Rgb(fr, fg, fb)) = (background, foreground) else {
        return background;
    };
    let channel = |base: u8, tint: u8| ((u16::from(base) * 6 + u16::from(tint)) / 7) as u8;
    Color::Rgb(channel(br, fr), channel(bg, fg), channel(bb, fb))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_custom_indexed_and_monochrome_keep_semantic_roles_distinct() {
        let night_owl = Theme::from_palette(&builtin_palette("night-owl").unwrap());
        assert_eq!(night_owl.background.bg, Some(Color::Rgb(1, 22, 39)));
        assert_ne!(night_owl.addition_line.fg, night_owl.deletion_line.fg);
        assert_ne!(night_owl.addition_line.bg, night_owl.background.bg);
        assert_ne!(night_owl.deletion_line.bg, night_owl.background.bg);
        assert_ne!(night_owl.focus.fg, night_owl.important.fg);

        let mut custom = builtin_palette("tokyo-night").unwrap();
        custom.focus = "#123456".into();
        let custom = Theme::from_palette(&custom);
        assert_eq!(custom.focus.fg, Some(Color::Rgb(0x12, 0x34, 0x56)));

        let indexed = Theme::indexed("night-owl");
        assert!(matches!(indexed.addition_line.fg, Some(Color::Indexed(_))));
        assert_ne!(indexed.addition_line.fg, indexed.deletion_line.fg);

        let monochrome = Theme::monochrome();
        assert!(
            monochrome
                .selected
                .add_modifier
                .contains(Modifier::REVERSED)
        );
        assert!(monochrome.error.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn selector_lists_builtins_then_custom_themes_without_overriding_presets() {
        let palette = builtin_palette("night-owl").unwrap();
        let settings = TuiSettings {
            theme: "night-owl".into(),
            themes: BTreeMap::from([
                ("night-owl".into(), palette.clone()),
                ("workbench".into(), palette),
            ]),
            syntax_highlighting: true,
        };

        assert_eq!(
            available(&settings),
            vec![
                "night-owl",
                "tokyo-night",
                "catppuccin-mocha",
                "terminal",
                "workbench"
            ]
        );
    }
}
