//! Nerd Font 字形：文件 / 目录 / 网页的类型图标。
//!
//! 字体 `JetBrains Maple Mono`（18MB）自带完整 Nerd Font 图标集，所以文件类图标
//! **不需要任何图片**：直接往 text 元素里塞一个 PUA 码点，和画 `❯` / `★` 一样。
//!
//! 码点来自 **eza**（`src/output/icons.rs`）的图标表 —— 那是长期打磨过的选择：
//! 按**语言**给专属图标（Rust 齿轮、Python、Go、JS/TS…），比自己拍脑袋的
//! 「所有代码一个 `</>`」辨识度高得多。yazi 的 `theme-dark.toml` 用的是同一套 PUA，
//! 但它主要按「文件名」配（`.gitignore`、`node_modules`），对启动器用不上。
//!
//! 换字体后要重量一次（字体里没有的字形会显示成豆腐块）：用 PowerShell + GDI+
//! 把一批码点画成 PNG 逐个看一眼即可（`System.Drawing` + `DrawString`，字号 30）。
//! 已知坑：老的 `F60F`（fa-markdown）在这个字体里是斜线方块，用 oct 的 `F48A`。

use std::path::Path;

// 通用文件类型
pub const FOLDER: char = '\u{E5FF}';
pub const FILE: char = '\u{F15B}';
pub const TEXT: char = '\u{F15C}';
pub const MARKDOWN: char = '\u{F48A}';
pub const PDF: char = '\u{F1C1}';
pub const ARCHIVE: char = '\u{F410}';
pub const IMAGE: char = '\u{F1C5}';
pub const SHEET: char = '\u{F1C3}';
pub const SLIDE: char = '\u{F1C4}';
pub const DOCUMENT: char = '\u{F1C2}';
pub const DATABASE: char = '\u{F1C0}';
pub const AUDIO: char = '\u{F001}';
pub const VIDEO: char = '\u{F03D}';
pub const LOG: char = '\u{F18D}';
pub const NOTEBOOK: char = '\u{E678}';
pub const DISK_IMAGE: char = '\u{E271}';

// 配置 / 元数据
pub const CONFIG: char = '\u{F107B}';
pub const ENV: char = '\u{F462}';
pub const LOCK: char = '\u{F023}';
pub const KEY: char = '\u{EB11}';
pub const LICENSE: char = '\u{F02D}';
pub const GIT: char = '\u{F02A2}';

// 可执行 / 二进制
pub const EXEC: char = '\u{EBC4}';
pub const LIBRARY: char = '\u{EB9C}';
pub const BINARY: char = '\u{EAE8}';

// 语言专属
pub const RUST: char = '\u{E68B}';
pub const PYTHON: char = '\u{E606}';
pub const GO: char = '\u{E65E}';
pub const JAVASCRIPT: char = '\u{E74E}';
pub const TYPESCRIPT: char = '\u{E628}';
pub const REACT: char = '\u{E7BA}';
pub const C: char = '\u{E61E}';
pub const CPP: char = '\u{E61D}';
pub const CSHARP: char = '\u{F031B}';
pub const JAVA: char = '\u{E256}';
pub const KOTLIN: char = '\u{E634}';
pub const LUA: char = '\u{E620}';
pub const PHP: char = '\u{E73D}';
pub const RUBY: char = '\u{E739}';
pub const HASKELL: char = '\u{E777}';
pub const ASSEMBLY: char = '\u{E637}';
pub const SHELL: char = '\u{F1183}';
pub const SHELL_CMD: char = '\u{F489}';
pub const POWERSHELL: char = '\u{EBC7}';
pub const HTML: char = '\u{F13B}';
pub const CSS: char = '\u{E749}';
pub const SASS: char = '\u{E603}';
pub const JSON: char = '\u{E60B}';
pub const YAML: char = '\u{E8EB}';
pub const TOML: char = '\u{E6B2}';
pub const XML: char = '\u{F05C0}';
pub const CODE: char = '\u{F121}';

/// 网页：`[gui] web_icons = "globe"` 时用它；favicon 没拉下来 / 拉失败也回落到它
pub const GLOBE: char = '\u{F0AC}';

// ---- 非文件类的条目（按 Kind / Action 分流）----
/// 终端 —— `:run` / `:go:yazi` / `:go:pi`
pub const TERMINAL: char = '\u{F120}';
/// 齿轮 —— `:cfg` / `:set` 的动作项
pub const COG: char = '\u{F013}';
/// 开关 —— `:set` 里的开关 / 选项（和齿轮区分开）
pub const TOGGLE: char = '\u{F205}';
/// 服务器 —— `:blog` 的本地预览 / 管理页
pub const SERVER: char = '\u{F233}';
/// 停止 —— `:blog:stop` / 停本地服务
pub const STOP: char = '\u{F28D}';
/// 铅笔 —— `:blog:new`
pub const PENCIL: char = '\u{F044}';
/// 时钟 —— 老历史条目（没有快照、只能回填输入行的那些）
pub const CLOCK: char = '\u{F43A}';
/// 问号 —— 命令列表（`:` 打出来的那些）
pub const QUESTION: char = '\u{F059}';
/// 右箭头 —— 其余 `FillInput` 条目
pub const ARROW: char = '\u{F054}';
/// 放大镜 —— `:all`（应用 + 历史混排的默认搜索）
pub const SEARCH: char = '\u{F002}';
/// 星标 —— `:fav` 收藏
pub const STAR: char = '\u{F005}';

/// provider 关键字 → 字形。
///
/// 给 GUI 输入框那颗**原子 chip** 用：关键字一命中 provider，块头图标就用这里的字形。
/// 关键字表在 `providers::build()`（顺序即命令列表顺序），新增 provider 记得补一行，
/// 漏了只是回落到问号（没有编译错误可依赖）。
pub fn for_keyword(keyword: &str) -> char {
    match keyword {
        "app" => EXEC,
        "all" => SEARCH,
        "ev" => FILE,
        "z" => FOLDER,
        "web" => GLOBE,
        "blog" => PENCIL,
        "fav" => STAR,
        "cfg" => COG,
        "set" => TOGGLE,
        "hist" => CLOCK,
        _ => QUESTION,
    }
}

/// 路径 → 字形（目录给文件夹，其余按扩展名）
pub fn for_path(path: &Path) -> char {
    if path.is_dir() {
        return FOLDER;
    }
    let ext = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    for_extension(ext)
}

/// 扩展名（大小写不敏感、不带点）→ 字形。
///
/// 认不出来的一律 [`FILE`]；宁可给个通用纸片，也别让整行没图标。
pub fn for_extension(ext: &str) -> char {
    match ext.to_ascii_lowercase().as_str() {
        // 文档
        "md" | "markdown" | "mdx" | "jmd" => MARKDOWN,
        "pdf" => PDF,
        "txt" | "text" => TEXT,
        "log" => LOG,
        "doc" | "docm" | "docx" | "odt" | "rtf" => DOCUMENT,
        "xls" | "xlsm" | "xlsx" | "ods" | "csv" | "tsv" => SHEET,
        "ppt" | "pptx" | "odp" => SLIDE,
        "ipynb" => NOTEBOOK,

        // 压缩 / 镜像
        "zip" | "7z" | "rar" | "gz" | "tar" | "xz" | "bz2" | "zst" | "lz4" | "lzma" | "z" => ARCHIVE,
        "iso" | "img" => DISK_IMAGE,

        // 图片 / 音视频
        "png" | "jpg" | "jpeg" | "jpe" | "gif" | "webp" | "bmp" | "svg" | "ico" | "avif" | "tif"
        | "tiff" | "heic" | "heif" | "psd" | "jxl" | "cr2" => IMAGE,
        "mp3" | "flac" | "wav" | "ogg" | "m4a" | "aac" | "opus" | "wma" | "cue" => AUDIO,
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "wmv" | "flv" | "m4v" | "m2ts" => VIDEO,

        // 语言专属（eza 的选法）
        "rs" => RUST,
        "py" | "pyi" | "pyw" => PYTHON,
        "go" => GO,
        "js" | "mjs" | "cjs" => JAVASCRIPT,
        "ts" | "mts" | "cts" => TYPESCRIPT,
        "jsx" | "tsx" => REACT,
        "c" | "h" | "m" | "inl" => C,
        "cc" | "cpp" | "cxx" | "c++" | "hpp" | "hh" | "hxx" | "h++" | "cp" => CPP,
        "cs" | "csx" | "csproj" => CSHARP,
        "java" | "jar" | "jad" => JAVA,
        "kt" | "kts" => KOTLIN,
        "lua" | "luac" | "luau" => LUA,
        "php" => PHP,
        "rb" | "erb" | "gem" | "gemspec" => RUBY,
        "hs" | "lhs" => HASKELL,
        "asm" | "s" | "nasm" => ASSEMBLY,
        "sh" | "bash" | "zsh" | "fish" | "ksh" => SHELL,
        "bat" | "cmd" | "csh" => SHELL_CMD,
        "ps1" | "psm1" | "psd1" => POWERSHELL,
        "html" | "htm" | "xhtml" => HTML,
        "css" => CSS,
        "scss" | "sass" | "less" | "styl" => SASS,
        "json" | "json5" | "jsonc" => JSON,
        "yaml" | "yml" => YAML,
        "toml" => TOML,
        "xml" | "xaml" | "plist" => XML,
        "sql" | "db" | "sqlite" | "sqlite3" | "mdb" | "accdb" => DATABASE,
        // 没有专属图标但明显是代码的语言：给通用的 `</>`
        "dart" | "zig" | "nim" | "scala" | "vue" | "svelte" | "ex" | "exs" | "erl" | "hrl"
        | "clj" | "cljs" | "elm" | "ml" | "mli" | "fs" | "fsx" | "jl" | "groovy" | "gradle"
        => CODE,

        // 配置 / 元数据
        "ini" | "conf" | "cfg" | "config" | "properties" => CONFIG,
        "env" => ENV,
        "lock" | "lck" => LOCK,
        "key" | "pem" | "cer" | "crt" | "gpg" | "kdbx" => KEY,
        "license" | "licence" => LICENSE,
        "gitignore" | "gitattributes" | "gitmodules" | "gitconfig" => GIT,

        // 可执行
        "exe" | "msi" | "com" | "scr" => EXEC,
        "dll" | "lib" | "so" | "dylib" | "a" => LIBRARY,
        "elf" | "bin" | "o" | "obj" | "class" | "pyc" => BINARY,

        _ => FILE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn extensions_map_to_their_own_glyph() {
        assert_eq!(for_extension("md"), MARKDOWN);
        assert_eq!(for_extension("MD"), MARKDOWN, "扩展名大小写不敏感");
        assert_eq!(for_extension("pdf"), PDF);
        assert_eq!(for_extension("zip"), ARCHIVE);
        assert_eq!(for_extension("xlsx"), SHEET);
        assert_eq!(for_extension(""), FILE);
        assert_eq!(for_extension("unknownext"), FILE);
    }

    #[test]
    fn languages_get_their_own_marks() {
        // 别退回「所有代码一个 </>」—— 这是参考 eza 的主要理由
        assert_eq!(for_extension("rs"), RUST);
        assert_eq!(for_extension("py"), PYTHON);
        assert_eq!(for_extension("go"), GO);
        assert_eq!(for_extension("ts"), TYPESCRIPT);
        assert_eq!(for_extension("js"), JAVASCRIPT);
        assert_ne!(for_extension("rs"), for_extension("py"));
    }

    #[test]
    fn directories_get_the_folder_glyph() {
        let dir = std::env::temp_dir();
        assert!(dir.is_dir());
        assert_eq!(for_path(&dir), FOLDER);

        // 不存在的路径不是目录 → 按扩展名走
        let missing = PathBuf::from(r"C:\definitely\not\here.md");
        assert_eq!(for_path(&missing), MARKDOWN);
    }

    #[test]
    fn every_glyph_is_in_the_private_use_area() {
        // Nerd Font 全在 PUA；掉到普通区段说明抄错码点了
        for glyph in [
            FOLDER, FILE, TEXT, MARKDOWN, PDF, ARCHIVE, IMAGE, SHEET, SLIDE, DOCUMENT, DATABASE,
            AUDIO, VIDEO, LOG, NOTEBOOK, DISK_IMAGE, CONFIG, ENV, LOCK, KEY, LICENSE, GIT, EXEC,
            LIBRARY, BINARY, RUST, PYTHON, GO, JAVASCRIPT, TYPESCRIPT, REACT, C, CPP, CSHARP,
            JAVA, KOTLIN, LUA, PHP, RUBY, HASKELL, ASSEMBLY, SHELL, SHELL_CMD, POWERSHELL, HTML,
            CSS, SASS, JSON, YAML, TOML, XML, CODE, GLOBE, TERMINAL, COG, TOGGLE,
            SERVER, STOP, PENCIL, CLOCK, QUESTION, ARROW,
        ] {
            let code = glyph as u32;
            assert!(
                (0xE000..=0xF8FF).contains(&code) || (0xF0000..=0xFFFFD).contains(&code),
                "{code:X} 不在 Nerd Font 的私用区"
            );
        }
    }
}
