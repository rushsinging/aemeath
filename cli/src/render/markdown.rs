use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
use crossterm::ExecutableCommand;
use std::io::{self, Write};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::{as_24_bit_terminal_escaped, LinesWithEndings};

pub fn render_markdown(text: &str) {
    let mut stdout = io::stdout();
    let mut in_code_block = false;
    let mut code_lang = String::new();
    let mut code_lines: Vec<String> = Vec::new();

    for line in text.lines() {
        if line.starts_with("```") {
            if in_code_block {
                render_code_block(&code_lines.join("\n"), &code_lang);
                code_lines.clear();
                code_lang.clear();
                in_code_block = false;
            } else {
                code_lang = line.trim_start_matches('`').trim().to_string();
                in_code_block = true;
            }
            continue;
        }

        if in_code_block {
            code_lines.push(line.to_string());
        } else {
            let rendered = termimad::inline(line);
            let _ = stdout.execute(Print(format!("{rendered}\n")));
        }
    }

    if in_code_block && !code_lines.is_empty() {
        render_code_block(&code_lines.join("\n"), &code_lang);
    }

    let _ = stdout.flush();
}

fn render_code_block(code: &str, lang: &str) {
    let ss = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    let theme = &ts.themes["base16-ocean.dark"];

    let syntax = ss
        .find_syntax_by_token(lang)
        .unwrap_or_else(|| ss.find_syntax_plain_text());

    let mut h = HighlightLines::new(syntax, theme);
    let mut stdout = io::stdout();

    let _ = stdout.execute(SetForegroundColor(Color::DarkGrey));
    let _ = stdout.execute(Print(format!("  ┌─ {lang}\n")));
    let _ = stdout.execute(ResetColor);

    for line in LinesWithEndings::from(code) {
        let ranges = h.highlight_line(line, &ss).unwrap_or_default();
        let escaped = as_24_bit_terminal_escaped(&ranges[..], false);
        print!("  │ {escaped}");
    }

    println!();

    let _ = stdout.execute(SetForegroundColor(Color::DarkGrey));
    let _ = stdout.execute(Print("  └─\n"));
    let _ = stdout.execute(ResetColor);
    let _ = stdout.flush();
}
