//! LaTeX-flavoured math to unicode, for inline and display math spans.


/// Render TeX math as well as one terminal line can: map the command spellings
/// the notes actually use to their unicode glyphs and keep everything else
/// verbatim — a wrong glyph is worse than legible source, and the editor is one
/// keypress away. Grouping braces are syntax and are dropped; `\frac{a}{b}`
/// becomes `a/b` so even a fraction stays on one line.
pub(super) fn tex_to_unicode(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '{' | '}' => i += 1,
            '~' => {
                out.push(' ');
                i += 1;
            }
            '\\' => {
                let start = i + 1;
                let mut end = start;
                while end < chars.len() && chars[end].is_ascii_alphabetic() {
                    end += 1;
                }
                if end > start {
                    let cmd: String = chars[start..end].iter().collect();
                    i = end;
                    match cmd.as_str() {
                        "frac" => {
                            if let Some((num, den, after)) = frac_terms(&chars, end) {
                                out.push_str(&tex_to_unicode(&num));
                                out.push('/');
                                out.push_str(&tex_to_unicode(&den));
                                i = after;
                            } else {
                                out.push_str("frac");
                            }
                            continue;
                        }
                        // Font-and-text commands just print the word they wrap:
                        // `\text{or}` is "or", and `\mathbb{R}` is "R".
                        "text" | "mbox" | "textrm" | "textup" | "mathrm" | "mathbf" | "mathit"
                        | "mathsf" | "mathtt" | "mathcal" | "mathbb" | "mathfrak"
                        | "operatorname" => {
                            if let Some((inner, after)) = take_braced(&chars, end) {
                                out.push_str(&tex_to_unicode(&inner));
                                i = after;
                            }
                            continue;
                        }
                        // Delimiter sizing — `\left(` means `(`, no more.
                        "left" | "right" | "big" | "Big" | "bigg" | "Bigg" | "bigl" | "bigr"
                        | "Bigl" | "Bigr" | "biggl" | "biggr" | "Biggl" | "Biggr" => continue,
                        _ => {}
                    }
                    match tex_command(&cmd) {
                        Some(glyph) => out.push_str(glyph),
                        None => {
                            out.push('\\');
                            out.push_str(&cmd);
                        }
                    }
                } else {
                    // One non-letter after the backslash: `\,`, `\ `, `\%`, `\\`.
                    i = match chars.get(start) {
                        None => {
                            out.push('\\');
                            start
                        }
                        Some(&c) => {
                            let esc: String = ['\\', c].iter().collect();
                            match esc.as_str() {
                                // Spacing commands all collapse to a space.
                                "\\," | "\\;" | "\\:" | "\\!" | "\\ " => out.push(' '),
                                "\\\\" => out.push('\n'),
                                "\\%" => out.push('%'),
                                "\\$" => out.push('$'),
                                "\\#" => out.push('#'),
                                "\\&" => out.push('&'),
                                "\\_" => out.push('_'),
                                "\\{" => out.push('{'),
                                "\\}" => out.push('}'),
                                _ => out.push_str(&esc),
                            }
                            start + 1
                        }
                    };
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// TeX command → terminal glyph. Only the spellings a note plausibly uses;
/// anything else stays verbatim in the rendered line.
fn tex_command(cmd: &str) -> Option<&'static str> {
    Some(match cmd {
        // arrows
        "to" | "rightarrow" => "→",
        "gets" | "leftarrow" => "←",
        "leftrightarrow" => "↔",
        "Rightarrow" | "implies" => "⇒",
        "Leftarrow" | "impliedby" => "⇐",
        "Leftrightarrow" | "iff" => "⇔",
        "uparrow" => "↑",
        "downarrow" => "↓",
        "updownarrow" => "↕",
        "Uparrow" => "⇑",
        "Downarrow" => "⇓",
        "Updownarrow" => "⇕",
        "mapsto" => "↦",
        "longrightarrow" => "⟶",
        "longleftarrow" => "⟵",
        "longleftrightarrow" => "⟷",
        "nearrow" => "↗",
        "searrow" => "↘",
        "swarrow" => "↙",
        "nwarrow" => "↖",
        "hookrightarrow" => "↪",
        "hookleftarrow" => "↩",
        "twoheadrightarrow" => "↠",
        "rightleftharpoons" => "⇌",
        // greek, lower
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "epsilon" => "ε",
        "varepsilon" => "ϵ",
        "zeta" => "ζ",
        "eta" => "η",
        "theta" => "θ",
        "vartheta" => "ϑ",
        "iota" => "ι",
        "kappa" => "κ",
        "lambda" => "λ",
        "mu" => "μ",
        "nu" => "ν",
        "xi" => "ξ",
        "pi" => "π",
        "varpi" => "ϖ",
        "rho" => "ρ",
        "varrho" => "ϱ",
        "sigma" => "σ",
        "varsigma" => "ς",
        "tau" => "τ",
        "upsilon" => "υ",
        "phi" => "φ",
        "varphi" => "ϕ",
        "chi" => "χ",
        "psi" => "ψ",
        "omega" => "ω",
        // greek, upper
        "Gamma" => "Γ",
        "Delta" => "Δ",
        "Theta" => "Θ",
        "Lambda" => "Λ",
        "Xi" => "Ξ",
        "Pi" => "Π",
        "Sigma" => "Σ",
        "Upsilon" => "Υ",
        "Phi" => "Φ",
        "Psi" => "Ψ",
        "Omega" => "Ω",
        // relations
        "neg" | "lnot" => "¬",
        "in" => "∈",
        "notin" => "∉",
        "ni" => "∋",
        "subset" => "⊂",
        "supset" => "⊃",
        "subseteq" => "⊆",
        "supseteq" => "⊇",
        "nsubseteq" => "⊈",
        "nsupseteq" => "⊉",
        "cup" => "∪",
        "cap" => "∩",
        "sqcup" => "⊔",
        "sqcap" => "⊓",
        "bigcup" => "⋃",
        "bigcap" => "⋂",
        "wedge" | "land" => "∧",
        "vee" | "lor" => "∨",
        "forall" => "∀",
        "exists" => "∃",
        "nexists" => "∄",
        "therefore" => "∴",
        "because" => "∵",
        "mid" => "∣",
        "nmid" => "∤",
        "parallel" => "∥",
        "perp" => "⊥",
        "leq" | "le" => "≤",
        "geq" | "ge" => "≥",
        "neq" | "ne" => "≠",
        "approx" => "≈",
        "equiv" => "≡",
        "sim" => "∼",
        "simeq" => "≃",
        "cong" => "≅",
        "propto" => "∝",
        // operators and misc
        "pm" => "±",
        "mp" => "∓",
        "times" => "×",
        "div" => "÷",
        "cdot" => "⋅",
        "cdots" => "⋯",
        "ldots" | "dots" => "…",
        "vdots" => "⋮",
        "ddots" => "⋱",
        "sum" => "∑",
        "prod" => "∏",
        "coprod" => "∐",
        "int" => "∫",
        "iint" => "∬",
        "iiint" => "∭",
        "oint" => "∮",
        "star" => "⋆",
        "ast" => "∗",
        "circ" => "∘",
        "bullet" => "•",
        "dagger" => "†",
        "ddagger" => "‡",
        "partial" => "∂",
        "nabla" => "∇",
        "infty" => "∞",
        "emptyset" | "varnothing" => "∅",
        "prime" => "′",
        "angle" => "∠",
        "triangle" => "△",
        "square" => "□",
        "surd" | "sqrt" => "√",
        "quad" => "  ",
        "qquad" => "    ",
        _ => return None,
    })
}

/// The next `{…}` group as its (nested-aware) inner text plus the index just
/// past the close, or `None` if the next character is not `{`.
fn take_braced(chars: &[char], at: usize) -> Option<(String, usize)> {
    if chars.get(at) != Some(&'{') {
        return None;
    }
    let mut depth = 0usize;
    let mut inner = String::new();
    for (k, &c) in chars.iter().enumerate().skip(at) {
        match c {
            '{' => {
                if depth > 0 {
                    inner.push('{');
                }
                depth += 1;
            }
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some((inner, k + 1));
                }
                inner.push('}');
            }
            _ => inner.push(c),
        }
    }
    None
}

/// `\frac`'s two groups, or `None` when either is missing.
fn frac_terms(chars: &[char], at: usize) -> Option<(String, String, usize)> {
    let (num, after) = take_braced(chars, at)?;
    let (den, end) = take_braced(chars, after)?;
    Some((num, den, end))
}
