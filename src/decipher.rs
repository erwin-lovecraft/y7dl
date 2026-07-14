//! Signature deciphering for cipher-protected stream URLs.
//!
//! Formats without a plain `url` carry a `signatureCipher` query blob
//! (`s=<scrambled>&sp=<param>&url=<stream>`). The scrambled signature must be
//! run through a per-player-version sequence of transforms (reverse / splice /
//! swap) extracted from YouTube's `base.js`, then appended to the stream URL.
//!
//! This is a hand-rolled string parser (no `regex` crate per project policy).
//! The `n`-parameter throttle transform is intentionally NOT ported: in the Go
//! original it requires executing player JS with the goja interpreter, and a
//! JS engine is outside this crate's allowed dependencies. Without it,
//! downloads of ciphered formats may be rate-limited but still work; the
//! default ANDROID_VR client returns direct URLs that need no deciphering.

use crate::error::{Error, Result};

/// One primitive signature transform, in base.js vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SigOp {
    /// `a.reverse()`
    Reverse,
    /// `a.splice(0,b)` — drop the first `b` chars.
    Splice(usize),
    /// `a[0] <-> a[b % a.len()]`
    Swap(usize),
}

/// Applies a parsed op sequence to a scrambled signature.
pub(crate) fn apply_ops(ops: &[SigOp], s: &str) -> String {
    let mut chars: Vec<char> = s.chars().collect();
    for op in ops {
        match *op {
            SigOp::Reverse => chars.reverse(),
            SigOp::Splice(n) => {
                chars.drain(..n.min(chars.len()));
            }
            SigOp::Swap(n) => {
                if !chars.is_empty() {
                    let j = n % chars.len();
                    chars.swap(0, j);
                }
            }
        }
    }
    chars.into_iter().collect()
}

/// Finds the player JS path (`/s/player/<ver>/.../base.js`) in an embed page.
/// Skips `/s/player/` references to other assets (e.g. `www-player.css`).
pub(crate) fn find_player_js_path(html: &str) -> Option<String> {
    let mut search = html;
    loop {
        let start = search.find("/s/player/")?;
        let rest = &search[start..];
        let end = rest.find("base.js")? + "base.js".len();
        let path = &rest[..end];
        // A real path is one token; reject matches spanning quotes/whitespace.
        if !path
            .chars()
            .any(|c| c == '"' || c == '\'' || c == '\\' || c.is_whitespace())
        {
            return Some(path.to_owned());
        }
        search = &search[start + "/s/player/".len()..];
    }
}

/// Extracts the ordered signature transforms from player JS.
///
/// The player contains a decipher function shaped like
/// `function(a){a=a.split("");Xr.zO(a,2);Xr.w1(a,5);...;return a.join("")}`
/// (newer players use `a.split(a.slice(0,0))`) plus a helper object
/// `var Xr={zO:function(a){a.reverse()},...}` whose members are classified by
/// body: `reverse` / `splice` / index swap.
pub(crate) fn parse_signature_ops(js: &str) -> Result<Vec<SigOp>> {
    let cipher_err = |msg: &str| Error::Cipher(msg.to_owned());

    // Locate the decipher function body via its `a=a.split(...)` prologue.
    let split_pos = js
        .find("=function(a){a=a.split(")
        .ok_or_else(|| cipher_err("decipher function not found in player js"))?;
    let body_start = js[split_pos..]
        .find(';')
        .map(|i| split_pos + i + 1)
        .ok_or_else(|| cipher_err("malformed decipher function"))?;
    let body_end = js[body_start..]
        .find("return a.join(")
        .map(|i| body_start + i)
        .ok_or_else(|| cipher_err("decipher function has no join()"))?;
    let body = &js[body_start..body_end];

    // Statements look like `Xr.zO(a,3)`; grab the helper object's name first.
    let obj_name = body
        .split_once('.')
        .map(|(name, _)| name.trim_start_matches(';').trim())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| cipher_err("no transform calls in decipher function"))?;

    let helpers = parse_helper_object(js, obj_name)?;

    let mut ops = Vec::new();
    for stmt in body.split(';').filter(|s| !s.trim().is_empty()) {
        let stmt = stmt.trim();
        // `Xr.zO(a,3)` → fn name `zO`, arg 3.
        let after_dot = stmt
            .split_once('.')
            .map(|(_, rest)| rest)
            .ok_or_else(|| cipher_err("unexpected statement in decipher function"))?;
        let (fn_name, args) = after_dot
            .split_once('(')
            .ok_or_else(|| cipher_err("unexpected statement in decipher function"))?;
        let arg: usize = args
            .trim_end_matches(')')
            .split(',')
            .nth(1)
            .and_then(|n| n.trim().parse().ok())
            .unwrap_or(0);

        let kind = helpers
            .iter()
            .find(|(name, _)| name == fn_name)
            .map(|(_, kind)| *kind)
            .ok_or_else(|| cipher_err("decipher call references unknown helper"))?;

        ops.push(match kind {
            HelperKind::Reverse => SigOp::Reverse,
            HelperKind::Splice => SigOp::Splice(arg),
            HelperKind::Swap => SigOp::Swap(arg),
        });
    }

    if ops.is_empty() {
        return Err(cipher_err("decipher function contained no transforms"));
    }
    Ok(ops)
}

#[derive(Debug, Clone, Copy)]
enum HelperKind {
    Reverse,
    Splice,
    Swap,
}

/// Parses `var <name>={fn1:function(a){...},fn2:function(a,b){...},...}` and
/// classifies each member function by its body.
fn parse_helper_object(js: &str, name: &str) -> Result<Vec<(String, HelperKind)>> {
    let cipher_err = |msg: &str| Error::Cipher(msg.to_owned());

    let decl = format!("var {name}={{");
    let obj_start = js
        .find(&decl)
        .map(|i| i + decl.len())
        .ok_or_else(|| cipher_err("helper object not found in player js"))?;

    let mut helpers = Vec::new();
    let mut rest = &js[obj_start..];
    loop {
        rest = rest.trim_start_matches([',', '\n', '\r', ' ']);
        if rest.starts_with('}') || rest.is_empty() {
            break;
        }
        let colon = rest
            .find(':')
            .ok_or_else(|| cipher_err("malformed helper object"))?;
        let fn_name = rest[..colon].trim().to_owned();
        let brace = rest
            .find('{')
            .ok_or_else(|| cipher_err("malformed helper object"))?;
        // Helper bodies contain no nested braces, but scan defensively.
        let mut depth = 0usize;
        let mut body_end = None;
        for (i, c) in rest[brace..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        body_end = Some(brace + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let body_end = body_end.ok_or_else(|| cipher_err("malformed helper object"))?;
        let body = &rest[brace + 1..body_end];

        let kind = if body.contains("reverse(") {
            HelperKind::Reverse
        } else if body.contains("splice(") {
            HelperKind::Splice
        } else {
            HelperKind::Swap
        };
        helpers.push((fn_name, kind));
        rest = &rest[body_end + 1..];
    }

    if helpers.is_empty() {
        return Err(cipher_err("helper object contained no functions"));
    }
    Ok(helpers)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYER_JS: &str = r#"
var Xr={zO:function(a){a.reverse()},
pQ:function(a,b){a.splice(0,b)},
w1:function(a,b){var c=a[0];a[0]=a[b%a.length];a[b%a.length]=c}};
var dec=function(a){a=a.split("");Xr.pQ(a,2);Xr.zO(a,24);Xr.w1(a,3);return a.join("")};
"#;

    #[test]
    fn parses_ops_from_player_js() {
        let ops = parse_signature_ops(PLAYER_JS).unwrap();
        assert_eq!(ops, vec![SigOp::Splice(2), SigOp::Reverse, SigOp::Swap(3)]);
    }

    #[test]
    fn parses_ops_from_new_style_split() {
        let js = PLAYER_JS.replace(r#"a.split("")"#, "a.split(a.slice(0,0))");
        let ops = parse_signature_ops(&js).unwrap();
        assert_eq!(ops.len(), 3);
    }

    #[test]
    fn applies_ops_correctly() {
        // "abcdefg" → splice(2) → "cdefg" → reverse → "gfedc" → swap(3) → "dfegc"
        let ops = [SigOp::Splice(2), SigOp::Reverse, SigOp::Swap(3)];
        assert_eq!(apply_ops(&ops, "abcdefg"), "dfegc");
        // Degenerate inputs must not panic.
        assert_eq!(apply_ops(&ops, ""), "");
        assert_eq!(apply_ops(&[SigOp::Splice(10)], "abc"), "");
    }

    #[test]
    fn finds_player_js_path() {
        let html = r#"<link href="/s/player/abc123def/www-player.css" rel="stylesheet">"jsUrl":"/s/player/abc123def/player_ias.vflset/en_US/base.js","other":1"#;
        assert_eq!(
            find_player_js_path(html).as_deref(),
            Some("/s/player/abc123def/player_ias.vflset/en_US/base.js")
        );
        assert_eq!(find_player_js_path("no player here"), None);
    }

    #[test]
    fn missing_pieces_yield_cipher_errors() {
        assert!(matches!(
            parse_signature_ops("nothing useful"),
            Err(Error::Cipher(_))
        ));
        let js = r#"var dec=function(a){a=a.split("");Missing.fn(a,1);return a.join("")};"#;
        assert!(matches!(parse_signature_ops(js), Err(Error::Cipher(_))));
    }
}
