use nom::{
    IResult,
    Parser,
    bytes::{
        complete::tag,
        take,
    },
    character::{
        char,
        complete::digit0,
    },
    multi::separated_list0,
    sequence::preceded,
};
use rgb::Rgb;

pub enum Direction {
    Up,
    Down,
    Forward,
    Back,
}

pub enum EraseRegion {
    CursorToEnd,
    StartToCursor,
    StartToEnd,

    /// Includes Scrollback buffer; not valid on [`ControlSequence::EraseLine`]
    PrevToEnd,
}

pub enum ControlSequence {
    CursorMoveDir { dir: Direction, amt: usize },
    CursorMoveLine { dir: Direction, amt: usize },
    CursorSet { x: usize, y: Option<usize> },

    EraseDisplay(EraseRegion),
    EraseLine(EraseRegion),

    Scroll { dir: Direction, amt: usize },
    DeviceStatus,

    Sgi(SelectGraphicRendition),
}

#[allow(clippy::too_many_lines)] // FIXME
pub fn parse_control_sequence(sequence: &str) -> IResult<&str, ControlSequence> {
    preceded(
        tag("\x3B["),
        (
            separated_list0(char(';'), digit0.map(|v: &str| v.parse::<usize>().ok())),
            take(1usize),
        ),
    )
    .map_opt(|(parameters, opcode)| {
        let mut parameters = parameters.into_iter().chain(core::iter::repeat(None));

        match opcode {
            // CSI n A - Cursor Up (default 1)
            "A" => {
                let amt = parameters
                    .next()
                    .expect("Infinite Iterator shouldn't fail")
                    .unwrap_or(1);
                Some(ControlSequence::CursorMoveDir {
                    dir: Direction::Up,
                    amt,
                })
            },
            // CSI n B - Cursor Down (default 1)
            "B" => {
                let amt = parameters
                    .next()
                    .expect("Infinite Iterator shouldn't fail")
                    .unwrap_or(1);
                Some(ControlSequence::CursorMoveDir {
                    dir: Direction::Down,
                    amt,
                })
            },
            // CSI n C - Cursor Forward (default 1)
            "C" => {
                let amt = parameters
                    .next()
                    .expect("Infinite Iterator shouldn't fail")
                    .unwrap_or(1);
                Some(ControlSequence::CursorMoveDir {
                    dir: Direction::Forward,
                    amt,
                })
            },
            // CSI n D - Cursor Back (default 1)
            "D" => {
                let amt = parameters
                    .next()
                    .expect("Infinite Iterator shouldn't fail")
                    .unwrap_or(1);
                Some(ControlSequence::CursorMoveDir {
                    dir: Direction::Back,
                    amt,
                })
            },

            // CSI n E - Cursor Next Line (default 1)
            "E" => {
                let amt = parameters
                    .next()
                    .expect("Infinite Iterator shouldn't fail")
                    .unwrap_or(1);
                Some(ControlSequence::CursorMoveLine {
                    dir: Direction::Down,
                    amt,
                })
            },
            // CSI n F - Cursor Prev Line (default 1)
            "F" => {
                let amt = parameters
                    .next()
                    .expect("Infinite Iterator shouldn't fail")
                    .unwrap_or(1);
                Some(ControlSequence::CursorMoveLine {
                    dir: Direction::Up,
                    amt,
                })
            },

            // CSI n G - Cursor Horizontal Absolute (default 1)
            "G" => {
                let n = parameters
                    .next()
                    .expect("Infinite Iterator shouldn't fail")
                    .unwrap_or(1);
                Some(ControlSequence::CursorSet { x: n, y: None })
            },
            // CSI n ; m H - Cursor Position (default 1)
            "H" | "f" => {
                let n = parameters
                    .next()
                    .expect("Infinite Iterator shouldn't fail")
                    .unwrap_or(1);
                let m = parameters
                    .next()
                    .expect("Infinite Iterator shouldn't fail")
                    .unwrap_or(1);
                Some(ControlSequence::CursorSet { x: n, y: Some(m) })
            },

            // CSI n J - Erase In Display
            "J" => {
                let n = parameters
                    .next()
                    .expect("Infinite Iterator shouldn't fail")
                    .unwrap_or(0);
                match n {
                    0 => Some(ControlSequence::EraseDisplay(EraseRegion::CursorToEnd)),
                    1 => Some(ControlSequence::EraseDisplay(EraseRegion::StartToCursor)),
                    2 => Some(ControlSequence::EraseDisplay(EraseRegion::StartToEnd)),
                    3 => Some(ControlSequence::EraseDisplay(EraseRegion::PrevToEnd)),
                    _ => None,
                }
            },
            // CSI n J - Erase In Line
            "K" => {
                let n = parameters
                    .next()
                    .expect("Infinite Iterator shouldn't fail")
                    .unwrap_or(0);
                match n {
                    0 => Some(ControlSequence::EraseLine(EraseRegion::CursorToEnd)),
                    1 => Some(ControlSequence::EraseLine(EraseRegion::StartToCursor)),
                    2 => Some(ControlSequence::EraseLine(EraseRegion::StartToEnd)),
                    _ => None,
                }
            },

            // CSI n S - Scroll Up (default 1)
            "S" => {
                let amt = parameters
                    .next()
                    .expect("Infinite Iterator shouldn't fail")
                    .unwrap_or(1);
                Some(ControlSequence::Scroll {
                    dir: Direction::Up,
                    amt,
                })
            },
            // CSI n T - Scroll Down (default 1)
            "T" => {
                let amt = parameters
                    .next()
                    .expect("Infinite Iterator shouldn't fail")
                    .unwrap_or(1);
                Some(ControlSequence::Scroll {
                    dir: Direction::Down,
                    amt,
                })
            },

            // CSI 6n - Device Status Report
            "n" => Some(ControlSequence::DeviceStatus),
            "m" => SelectGraphicRendition::from_params(parameters).map(ControlSequence::Sgi),

            _ => None,
        }
    })
    .parse_complete(sequence)
}

pub enum SelectFont {
    Primary,
    Alt(usize),
    Gothic,
}

pub enum FontIntensity {
    Faint,
    Normal,
    Strong,
}

pub enum BlinkSpeed {
    Slow,
    Rapid,
    Off,
}

pub enum FontModifier {
    Italic,
    Underline,
    DoubleUnderline,
    Inverted,
    Concealed,
    Striked,
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum EgaColor {
    Black = 0,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
}

impl EgaColor {
    pub const fn new(v: usize) -> Option<Self> {
        match v {
            0 => Some(Self::Black),
            1 => Some(Self::Red),
            2 => Some(Self::Green),
            3 => Some(Self::Yellow),
            4 => Some(Self::Blue),
            5 => Some(Self::Magenta),
            6 => Some(Self::Cyan),
            7 => Some(Self::White),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum FontColor {
    Default,
    Ega(EgaColor),
    Lut(u8),
    TrueColor(Rgb<u8>),
}

impl FontColor {
    pub fn as_rgb(self) -> Rgb<u8> {
        match self {
            Self::TrueColor(color) => color,
            Self::Ega(ega_color) => {
                // According to CGA/EGA/VGA
                match ega_color {
                    EgaColor::Black => Rgb::new(0, 0, 0),
                    EgaColor::Red => Rgb::new(170, 0, 0),
                    EgaColor::Green => Rgb::new(0, 170, 0),
                    EgaColor::Yellow => Rgb::new(170, 85, 0),
                    EgaColor::Blue => Rgb::new(0, 0, 170),
                    EgaColor::Magenta => Rgb::new(170, 0, 170),
                    EgaColor::Cyan => Rgb::new(0, 170, 170),
                    EgaColor::White => Rgb::new(170, 170, 170),
                }
            },
            Self::Lut(_selector) => {
                todo!("Support Lut Font Colors")
            },
            Self::Default => Rgb::default(),
        }
    }
}

pub enum SelectGraphicRendition {
    Reset,
    Intensity(FontIntensity),
    SetModifier {
        modifier: FontModifier,
        value:    bool,
    },
    BlinkSpeed(BlinkSpeed),
    SetFont(SelectFont),

    Background(FontColor),
    Foreground(FontColor),
    Underline(FontColor),
}

impl SelectGraphicRendition {
    #[allow(clippy::too_many_lines)] // FIXME
    pub fn from_params(mut parameters: impl Iterator<Item = Option<usize>>) -> Option<Self> {
        let mode = parameters
            .next()
            .expect("Infinite Iterator shouldn't fail")
            .unwrap_or(0);
        match mode {
            0 => Some(Self::Reset),
            1 => Some(Self::Intensity(FontIntensity::Strong)),
            2 => Some(Self::Intensity(FontIntensity::Faint)),
            3 => Some(Self::SetModifier {
                modifier: FontModifier::Italic,
                value:    true,
            }),
            4 => Some(Self::SetModifier {
                modifier: FontModifier::Underline,
                value:    true,
            }),
            5 => Some(Self::BlinkSpeed(BlinkSpeed::Slow)),
            6 => Some(Self::BlinkSpeed(BlinkSpeed::Rapid)),
            7 => Some(Self::SetModifier {
                modifier: FontModifier::Inverted,
                value:    true,
            }),
            8 => Some(Self::SetModifier {
                modifier: FontModifier::Concealed,
                value:    true,
            }),
            9 => Some(Self::SetModifier {
                modifier: FontModifier::Striked,
                value:    true,
            }),
            10 => Some(Self::SetFont(SelectFont::Primary)),
            11..=19 => Some(Self::SetFont(SelectFont::Alt(mode - 11))),
            20 => Some(Self::SetFont(SelectFont::Gothic)),
            21 => Some(Self::SetModifier {
                modifier: FontModifier::DoubleUnderline,
                value:    true,
            }),
            22 => Some(Self::Intensity(FontIntensity::Normal)),
            23 => Some(Self::SetModifier {
                modifier: FontModifier::Italic,
                value:    false,
            }),
            24 => Some(Self::SetModifier {
                modifier: FontModifier::Underline,
                value:    false,
            }),
            25 => Some(Self::BlinkSpeed(BlinkSpeed::Off)),
            // 26 - Proportional Spacing
            27 => Some(Self::SetModifier {
                modifier: FontModifier::Inverted,
                value:    false,
            }),
            28 => Some(Self::SetModifier {
                modifier: FontModifier::Concealed,
                value:    false,
            }),
            29 => Some(Self::SetModifier {
                modifier: FontModifier::Striked,
                value:    false,
            }),

            // Foreground Color
            30..=37 => {
                let ega_color = EgaColor::new(mode - 30);
                ega_color.map(|color| Self::Foreground(FontColor::Ega(color)))
            },
            38 => {
                let color_mode = parameters
                    .next()
                    .expect("Infinite Iterator shouldn't fail")
                    .unwrap_or(0);
                match color_mode {
                    2 => {
                        let r = parameters
                            .next()
                            .expect("Infinite Iterator shouldn't fail")
                            .unwrap_or(0);
                        let g = parameters
                            .next()
                            .expect("Infinite Iterator shouldn't fail")
                            .unwrap_or(0);
                        let b = parameters
                            .next()
                            .expect("Infinite Iterator shouldn't fail")
                            .unwrap_or(0);

                        let Ok(r) = u8::try_from(r) else {
                            return None;
                        };
                        let Ok(g) = u8::try_from(g) else {
                            return None;
                        };
                        let Ok(b) = u8::try_from(b) else {
                            return None;
                        };

                        Some(Self::Foreground(FontColor::TrueColor(rgb::Rgb::new(
                            r, g, b,
                        ))))
                    },
                    5 => {
                        let selector = parameters
                            .next()
                            .expect("Infinite Iterator shouldn't fail")
                            .unwrap_or(0);

                        let Ok(selector) = u8::try_from(selector) else {
                            return None;
                        };

                        Some(Self::Foreground(FontColor::Lut(selector)))
                    },
                    _ => None,
                }
            },
            39 => Some(Self::Foreground(FontColor::Default)),

            // Background Color
            40..=47 => {
                let ega_color = EgaColor::new(mode - 40);
                ega_color.map(|color| Self::Background(FontColor::Ega(color)))
            },
            48 => {
                let color_mode = parameters
                    .next()
                    .expect("Infinite Iterator shouldn't fail")
                    .unwrap_or(0);
                match color_mode {
                    2 => {
                        let r = parameters
                            .next()
                            .expect("Infinite Iterator shouldn't fail")
                            .unwrap_or(0);
                        let g = parameters
                            .next()
                            .expect("Infinite Iterator shouldn't fail")
                            .unwrap_or(0);
                        let b = parameters
                            .next()
                            .expect("Infinite Iterator shouldn't fail")
                            .unwrap_or(0);

                        let Ok(r) = u8::try_from(r) else {
                            return None;
                        };
                        let Ok(g) = u8::try_from(g) else {
                            return None;
                        };
                        let Ok(b) = u8::try_from(b) else {
                            return None;
                        };

                        Some(Self::Background(FontColor::TrueColor(rgb::Rgb::new(
                            r, g, b,
                        ))))
                    },
                    5 => {
                        let selector = parameters
                            .next()
                            .expect("Infinite Iterator shouldn't fail")
                            .unwrap_or(0);

                        let Ok(selector) = u8::try_from(selector) else {
                            return None;
                        };

                        Some(Self::Background(FontColor::Lut(selector)))
                    },
                    _ => None,
                }
            },
            49 => Some(Self::Background(FontColor::Default)),

            // Underline Color
            58 => {
                let color_mode = parameters
                    .next()
                    .expect("Infinite Iterator shouldn't fail")
                    .unwrap_or(0);
                match color_mode {
                    2 => {
                        let r = parameters
                            .next()
                            .expect("Infinite Iterator shouldn't fail")
                            .unwrap_or(0);
                        let g = parameters
                            .next()
                            .expect("Infinite Iterator shouldn't fail")
                            .unwrap_or(0);
                        let b = parameters
                            .next()
                            .expect("Infinite Iterator shouldn't fail")
                            .unwrap_or(0);

                        let Ok(r) = u8::try_from(r) else {
                            return None;
                        };
                        let Ok(g) = u8::try_from(g) else {
                            return None;
                        };
                        let Ok(b) = u8::try_from(b) else {
                            return None;
                        };

                        Some(Self::Underline(FontColor::TrueColor(rgb::Rgb::new(
                            r, g, b,
                        ))))
                    },
                    5 => {
                        let selector = parameters
                            .next()
                            .expect("Infinite Iterator shouldn't fail")
                            .unwrap_or(0);

                        let Ok(selector) = u8::try_from(selector) else {
                            return None;
                        };

                        Some(Self::Underline(FontColor::Lut(selector)))
                    },
                    _ => None,
                }
            },
            59 => Some(Self::Underline(FontColor::Default)),

            _ => None,
        }
    }
}
