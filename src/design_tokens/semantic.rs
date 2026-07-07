//! SemanticColors：策展语义色，按亮/暗二选一，色值取 GitHub Primer（对比度已验证）。
//! 不再劫持终端 ANSI，upload 与 danger 脱钩（橙）。

use warpui_core::color::ColorU;

/// 全透明（平台无关常量），供闭包/const 上下文无捕获使用。
pub const TRANSPARENT: ColorU = ColorU {
    r: 0,
    g: 0,
    b: 0,
    a: 0,
};

/// Windows 关闭按钮 hover 红（平台惯例，非主题色）。
pub const WINDOWS_CLOSE_HOVER: ColorU = ColorU {
    r: 232,
    g: 17,
    b: 32,
    a: 0xff,
};

#[derive(Clone, Copy)]
pub struct SemanticColors {
    pub ok: ColorU,
    pub warn: ColorU,
    pub danger: ColorU,
    pub info: ColorU,
    pub memory: ColorU,
    pub swap: ColorU,
    pub upload: ColorU,
    pub download: ColorU,
    pub transparent: ColorU,
    pub windows_close_hover: ColorU,
}

impl SemanticColors {
    pub fn new(is_dark: bool) -> Self {
        if is_dark {
            Self {
                ok: rgb(0x3fb950),
                warn: rgb(0xd29922),
                danger: rgb(0xf85149),
                info: rgb(0x79a8ff),
                memory: rgb(0xd4a72c),
                swap: rgb(0xbc8cff),
                upload: rgb(0xf0883e),
                download: rgb(0x56d364),
                transparent: TRANSPARENT,
                windows_close_hover: WINDOWS_CLOSE_HOVER,
            }
        } else {
            Self {
                ok: rgb(0x1a7f37),
                warn: rgb(0x9a6700),
                danger: rgb(0xcf222e),
                info: rgb(0x0969da),
                memory: rgb(0xbf8700),
                swap: rgb(0x8250df),
                upload: rgb(0xbc4c00),
                download: rgb(0x2da44e),
                transparent: TRANSPARENT,
                windows_close_hover: WINDOWS_CLOSE_HOVER,
            }
        }
    }
}

const fn rgb(hex: u32) -> ColorU {
    ColorU {
        r: ((hex >> 16) & 0xff) as u8,
        g: ((hex >> 8) & 0xff) as u8,
        b: (hex & 0xff) as u8,
        a: 0xff,
    }
}
