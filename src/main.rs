// File: src/main.rs
// ==============================================================================
use clap::Parser as ClapParser;
use serde::Deserialize;
use similar::TextDiff;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use tree_sitter::{Node, Parser as TsParser};

#[derive(ClapParser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the JSON patch file containing the Aider code diff payload
    #[arg(short, long)]
    patch: PathBuf,

    /// Target working directory of the project workspace
    #[arg(short, long, default_value = ".")]
    cwd: PathBuf,
}

#[derive(Deserialize, Debug)]
struct PatchFileEntry {
    file_path: String,
    code_diff: String,
}

#[derive(Deserialize, Debug)]
struct PatchJsonStructure {
    summary: Option<String>,
    files: Vec<PatchFileEntry>,
}

fn prep(content: &str) -> (String, Vec<String>) {
    let mut adjusted = content.to_string();
    if !adjusted.is_empty() && !adjusted.ends_with('\n') {
        adjusted.push('\n');
    }
    let lines: Vec<String> = adjusted
        .split_inclusive('\n')
        .map(|s| s.to_string())
        .collect();
    (adjusted, lines)
}

fn perfect_replace(
    whole_lines: &[String],
    part_lines: &[String],
    replace_lines: &[String],
) -> Option<String> {
    let part_len = part_lines.len();
    if part_len == 0 {
        let mut res = replace_lines.join("");
        res.push_str(&whole_lines.join(""));
        return Some(res);
    }

    if whole_lines.len() < part_len {
        return None;
    }

    for i in 0..=(whole_lines.len() - part_len) {
        let mut matched = true;
        for j in 0..part_len {
            if whole_lines[i + j] != part_lines[j] {
                matched = false;
                break;
            }
        }
        if matched {
            let mut res = Vec::new();
            res.extend_from_slice(&whole_lines[0..i]);
            res.extend_from_slice(replace_lines);
            res.extend_from_slice(&whole_lines[i + part_len..]);
            return Some(res.join(""));
        }
    }
    None
}

fn match_but_for_leading_whitespace(whole: &[String], part: &[String]) -> Option<String> {
    let num = whole.len();
    if num != part.len() {
        return None;
    }

    for i in 0..num {
        if whole[i].trim_start() != part[i].trim_start() {
            return None;
        }
    }

    let mut additions = HashSet::new();
    for i in 0..num {
        if !whole[i].trim().is_empty() {
            let w_lead_len = whole[i].len() - whole[i].trim_start().len();
            additions.insert(whole[i][0..w_lead_len].to_string());
        }
    }

    if additions.len() != 1 {
        return None;
    }
    Some(additions.into_iter().next().unwrap_or_default())
}

fn replace_part_with_missing_leading_whitespace(
    whole_lines: &[String],
    part_lines: &[String],
    replace_lines: &[String],
) -> Option<String> {
    if part_lines.is_empty() {
        return None;
    }

    let mut leading = Vec::new();
    for p in part_lines {
        if !p.trim().is_empty() {
            leading.push(p.len() - p.trim_start().len());
        }
    }
    for r in replace_lines {
        if !r.trim().is_empty() {
            leading.push(r.len() - r.trim_start().len());
        }
    }

    let min_leading = leading.into_iter().min().unwrap_or(0);
    let mut adjusted_part_lines = part_lines.to_vec();
    let mut adjusted_replace_lines = replace_lines.to_vec();

    if min_leading > 0 {
        adjusted_part_lines = part_lines
            .iter()
            .map(|p| {
                if !p.trim().is_empty() {
                    p[min_leading..].to_string()
                } else {
                    p.clone()
                }
            })
            .collect();
        adjusted_replace_lines = replace_lines
            .iter()
            .map(|r| {
                if !r.trim().is_empty() {
                    r[min_leading..].to_string()
                } else {
                    r.clone()
                }
            })
            .collect();
    }

    let num_part_lines = adjusted_part_lines.len();
    if whole_lines.len() < num_part_lines {
        return None;
    }

    for i in 0..=(whole_lines.len() - num_part_lines) {
        if let Some(add_leading) = match_but_for_leading_whitespace(
            &whole_lines[i..i + num_part_lines],
            &adjusted_part_lines,
        ) {
            let adjusted_replace: Vec<String> = adjusted_replace_lines
                .iter()
                .map(|rline| {
                    if !rline.trim().is_empty() {
                        let mut l = add_leading.clone();
                        l.push_str(rline);
                        l
                    } else {
                        rline.clone()
                    }
                })
                .collect();

            let mut res = Vec::new();
            res.extend_from_slice(&whole_lines[0..i]);
            res.extend_from_slice(&adjusted_replace);
            res.extend_from_slice(&whole_lines[i + num_part_lines..]);
            return Some(res.join(""));
        }
    }
    None
}

fn try_dotdotdots(whole: &str, part: &str, replace: &str) -> Option<String> {
    if !part.contains("...") || !replace.contains("...") {
        return None;
    }

    let split_by_dots =
        |content: &str| -> Vec<String> { content.split("...").map(|s| s.to_string()).collect() };

    let part_pieces = split_by_dots(part);
    let replace_pieces = split_by_dots(replace);

    if part_pieces.len() != replace_pieces.len() || part_pieces.len() <= 1 {
        return None;
    }

    let mut result = whole.to_string();
    for i in 0..part_pieces.len() {
        let p = &part_pieces[i];
        let r = &replace_pieces[i];
        if p.is_empty() && r.is_empty() {
            continue;
        }
        if p.is_empty() && !r.is_empty() {
            if !result.ends_with('\n') {
                result.push('\n');
            }
            result.push_str(r);
            continue;
        }

        let first_occurrence = result.find(p)?;
        if result[first_occurrence + p.len()..].contains(p) {
            return None; // Ensure structural segment matches are unambiguous and unique
        }
        result = result.replace(p, r);
    }
    Some(result)
}

fn replace_closest_edit_distance(
    whole_lines: &[String],
    part: &str,
    part_lines: &[String],
    replace_lines: &[String],
) -> Option<String> {
    if part_lines.is_empty() || part_lines.len() > 100 {
        return None; // Prevent excessive window comparisons on heavy blocks
    }

    let similarity_thresh = 0.85;
    let mut max_similarity = 0.0;
    let mut best_start = 0;
    let mut best_end = 0;

    let scale = 0.1;
    let min_len = ((part_lines.len() as f64) * (1.0 - scale)).floor() as usize;
    let max_len = ((part_lines.len() as f64) * (1.0 + scale)).ceil() as usize;

    for length in min_len..=max_len {
        if whole_lines.len() < length {
            continue;
        }
        for i in 0..=(whole_lines.len() - length) {
            let chunk = &whole_lines[i..i + length];
            let chunk_str = chunk.join("");

            // Leverage .as_str() to force the generic TextDiff function to evaluate matching &str slices
            let ratio = TextDiff::from_lines(chunk_str.as_str(), part).ratio();
            if ratio > max_similarity {
                max_similarity = ratio;
                best_start = i;
                best_end = i + length;
            }
        }
    }

    if max_similarity < similarity_thresh {
        return None;
    }

    let mut res = Vec::new();
    res.extend_from_slice(&whole_lines[0..best_start]);
    res.extend_from_slice(replace_lines);
    res.extend_from_slice(&whole_lines[best_end..]);
    Some(res.join(""))
}

fn replace_most_similar_chunk(
    whole: &str,
    part: &str,
    replace: &str,
) -> (Option<String>, &'static str) {
    if part.trim().is_empty() {
        if whole.trim().is_empty() {
            return (Some(replace.to_string()), "New file initialization");
        }
        let mut suffix = whole.trim_end().to_string();
        suffix.push('\n');
        suffix.push_str(replace);
        suffix.push('\n');
        return (Some(suffix), "Empty search block append");
    }

    let (prep_whole, whole_lines) = prep(whole);
    let (prep_part, part_lines) = prep(part);
    let (_, replace_lines) = prep(replace);

    if let Some(exact) = perfect_replace(&whole_lines, &part_lines, &replace_lines) {
        return (Some(exact), "Exact match (Tier 1)");
    }

    if let Some(indent) =
        replace_part_with_missing_leading_whitespace(&whole_lines, &part_lines, &replace_lines)
    {
        return (Some(indent), "Indentation-adjusted match (Tier 2)");
    }

    if part_lines.len() > 1 && part_lines[0].trim().is_empty() {
        if let Some(skipped) = replace_part_with_missing_leading_whitespace(
            &whole_lines,
            &part_lines[1..],
            &replace_lines,
        ) {
            return (Some(skipped), "Skipped leading blank line match (Tier 2.1)");
        }
    }

    if let Some(dots) = try_dotdotdots(&prep_whole, &prep_part, replace) {
        return (Some(dots), "Elision (...) match (Tier 2.5)");
    }

    if let Some(fuzzy) =
        replace_closest_edit_distance(&whole_lines, &prep_part, &part_lines, &replace_lines)
    {
        return (Some(fuzzy), "Fuzzy sequence match (Tier 3)");
    }

    (None, "Failed to match")
}

fn find_closest_match_context(whole: &str, part: &str) -> String {
    let (_, whole_lines) = prep(whole);
    let (_, part_lines) = prep(part);

    if whole_lines.is_empty() || part_lines.is_empty() {
        return whole.chars().take(500).collect();
    }

    let mut max_similarity = -1.0;
    let mut best_start = 0;
    let part_str = part_lines.join("");
    let part_len = part_lines.len().min(whole_lines.len());

    for i in 0..=(whole_lines.len() - part_len) {
        let chunk = &whole_lines[i..i + part_len];
        let chunk_str = chunk.join("");
        let ratio = TextDiff::from_lines(chunk_str.as_str(), part_str.as_str()).ratio();
        if ratio > max_similarity {
            max_similarity = ratio;
            best_start = i;
        }
    }

    let block_center = best_start + (part_len / 2);
    let window_start = block_center.saturating_sub(2);
    let window_end = (block_center + 3).min(whole_lines.len());
    whole_lines[window_start..window_end].join("")
}

fn find_identifier_name(n: Node, src: &[u8]) -> Option<String> {
    let ck = n.kind();
    if ck == "identifier" || ck == "type_identifier" || ck == "property_identifier" {
        if let Ok(text) = n.utf8_text(src) {
            return Some(text.to_string());
        }
    }
    for i in 0..n.child_count() {
        if let Some(child) = n.child(i) {
            if let Some(name) = find_identifier_name(child, src) {
                return Some(name);
            }
        }
    }
    None
}

fn find_declared_entities(node: Node, source: &[u8], is_rust: bool) -> Vec<(String, String)> {
    let mut entities = Vec::new();

    fn traverse(n: Node, src: &[u8], ents: &mut Vec<(String, String)>, is_rust: bool) {
        let t = n.kind();
        let is_target = if is_rust {
            t == "function_item"
                || t == "struct_item"
                || t == "enum_item"
                || t == "trait_item"
                || t == "impl_item"
                || t == "mod_item"
        } else {
            t == "function_declaration"
                || t == "class_declaration"
                || t == "interface_declaration"
                || t == "method_definition"
                || t == "generator_function_declaration"
        };

        if is_target {
            if let Some(name) = find_identifier_name(n, src) {
                ents.push((t.to_string(), name));
            }
        }
        let mut cursor = n.walk();
        for child in n.children(&mut cursor) {
            traverse(child, src, ents, is_rust);
        }
    }

    traverse(node, source, &mut entities, is_rust);
    entities
}

fn find_matching_node<'a>(
    root_node: Node<'a>,
    kind: &str,
    name: &str,
    source: &[u8],
) -> Option<Node<'a>> {
    let mut matched = None;

    fn traverse<'a>(n: Node<'a>, k: &str, nm: &str, src: &[u8], res: &mut Option<Node<'a>>) {
        if res.is_some() {
            return;
        }
        if n.kind() == k {
            if let Some(node_name) = find_identifier_name(n, src) {
                if node_name == nm {
                    *res = Some(n);
                    return;
                }
            }
        }
        let mut cursor = n.walk();
        for child in n.children(&mut cursor) {
            traverse(child, k, nm, src, res);
        }
    }

    traverse(root_node, kind, name, source, &mut matched);
    matched
}

fn replace_via_ast_fallback(
    parser: &mut TsParser,
    current_content: &str,
    search_part: &str,
    replacement: &str,
    is_rust: bool,
) -> Option<String> {
    let whole_tree = parser.parse(current_content, None).unwrap();
    let replacement_tree = parser.parse(replacement, None).unwrap();
    let search_tree = parser.parse(search_part, None).unwrap();

    let mut replacement_entities = find_declared_entities(
        replacement_tree.root_node(),
        replacement.as_bytes(),
        is_rust,
    );
    let mut search_entities =
        find_declared_entities(search_tree.root_node(), search_part.as_bytes(), is_rust);

    // Try wrapping the block if we are replacing raw methods without explicit parent braces
    if replacement_entities.is_empty() && search_entities.is_empty() {
        let wrapped_replacement = if is_rust {
            format!("impl _DummyImpl {{\n{}\n}}", replacement)
        } else {
            format!("class _DummyClass {{\n{}\n}}", replacement)
        };
        let wrapped_search = if is_rust {
            format!("impl _DummyImpl {{\n{}\n}}", search_part)
        } else {
            format!("class _DummyClass {{\n{}\n}}", search_part)
        };
        let rep_tree_wrapped = parser.parse(&wrapped_replacement, None).unwrap();
        let s_tree_wrapped = parser.parse(&wrapped_search, None).unwrap();

        replacement_entities = find_declared_entities(
            rep_tree_wrapped.root_node(),
            wrapped_replacement.as_bytes(),
            is_rust,
        );
        search_entities = find_declared_entities(
            s_tree_wrapped.root_node(),
            wrapped_search.as_bytes(),
            is_rust,
        );
    }

    let all_entities = [replacement_entities, search_entities].concat();
    let mut target_node = None;

    for (kind, name) in &all_entities {
        target_node = find_matching_node(
            whole_tree.root_node(),
            kind,
            name,
            current_content.as_bytes(),
        );
        if target_node.is_some() {
            break;
        }
    }

    if let Some(node) = target_node {
        let mut node_to_replace = node;
        if !is_rust {
            if let Some(parent) = node.parent() {
                if parent.kind() == "export_statement" {
                    node_to_replace = parent;
                }
            }
        }

        let start_byte = node_to_replace.start_byte();
        let end_byte = node_to_replace.end_byte();

        let mut updated = current_content[0..start_byte].to_string();
        updated.push_str(replacement);
        updated.push_str(&current_content[end_byte..]);

        return Some(updated);
    }
    None
}

fn parse_diff_blocks(diff_text: &str) -> Vec<(String, String)> {
    let mut blocks = Vec::new();
    let lines: Vec<&str> = diff_text.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();
        if line.starts_with("<<<<<<<") && line.contains("SEARCH") {
            let mut search_lines = Vec::new();
            i += 1;
            while i < lines.len() {
                let next_line = lines[i];
                if next_line.trim().starts_with("=======") {
                    break;
                }
                search_lines.push(next_line);
                i += 1;
            }

            if i >= lines.len() {
                break;
            }

            let mut replace_lines = Vec::new();
            i += 1;
            while i < lines.len() {
                let next_line = lines[i];
                if next_line.trim().starts_with(">>>>>>>") && next_line.contains("REPLACE") {
                    break;
                }
                replace_lines.push(next_line);
                i += 1;
            }

            blocks.push((search_lines.join("\n"), replace_lines.join("\n")));
        }
        i += 1;
    }
    blocks
}

fn main() {
    let args = Args::parse();

    let patch_content = match fs::read_to_string(&args.patch) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("FATAL ERROR: Failed to read patch JSON file: {}", e);
            std::process::exit(1);
        }
    };

    let patch_data: PatchJsonStructure = match serde_json::from_str(&patch_content) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("FATAL ERROR: Failed to parse patch JSON payload: {}", e);
            std::process::exit(1);
        }
    };

    if let Some(summary) = &patch_data.summary {
        println!("🤖 Summary: {}", summary);
    }

    let mut file_updates = std::collections::HashMap::new();

    // Configure the Tree-Sitter TSX language parser globally
    let mut tsx_parser = TsParser::new();
    let tsx_language = tree_sitter_typescript::LANGUAGE_TSX;
    if tsx_parser.set_language(&tsx_language.into()).is_err() {
        eprintln!("FATAL ERROR: Failed to initialize tree-sitter typescript grammar.");
        std::process::exit(1);
    }

    // Configure the Tree-Sitter Rust language parser globally
    let mut rust_parser = TsParser::new();
    let rust_language = tree_sitter_rust::LANGUAGE;
    if rust_parser.set_language(&rust_language.into()).is_err() {
        eprintln!("FATAL ERROR: Failed to initialize tree-sitter rust grammar.");
        std::process::exit(1);
    }

    let mut errors_found = false;

    for file_info in &patch_data.files {
        let raw_path = &file_info.file_path;
        let file_path = if raw_path.starts_with('/') {
            PathBuf::from(raw_path)
        } else {
            args.cwd.join(raw_path)
        };

        let file_exists = file_path.exists();
        let content = if file_exists {
            match fs::read_to_string(&file_path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!(
                        "FATAL ERROR in file: {}\nCould not read file: {}",
                        raw_path, e
                    );
                    errors_found = true;
                    continue;
                }
            }
        } else {
            String::new()
        };

        let is_rust = raw_path.ends_with(".rs");
        let is_js_or_ts = raw_path.ends_with(".ts")
            || raw_path.ends_with(".tsx")
            || raw_path.ends_with(".js")
            || raw_path.ends_with(".jsx");

        let mut active_parser = if is_rust {
            Some(&mut rust_parser)
        } else if is_js_or_ts {
            Some(&mut tsx_parser)
        } else {
            None
        };

        let blocks = parse_diff_blocks(&file_info.code_diff);
        if blocks.is_empty() {
            eprintln!(
                "WARNING: No valid SEARCH/REPLACE blocks parsed for file: {}",
                raw_path
            );
            continue;
        }

        let mut current_content = content;
        for (idx, (search_part, replacement)) in blocks.iter().enumerate() {
            let (mut new_content, mut strategy) =
                replace_most_similar_chunk(&current_content, search_part, replacement);

            if new_content.is_none() {
                if let Some(ref mut parser) = active_parser {
                    if let Some(ast_updated) = replace_via_ast_fallback(
                        parser,
                        &current_content,
                        search_part,
                        replacement,
                        is_rust,
                    ) {
                        new_content = Some(ast_updated);
                        strategy = "AST-Node Replacement (Tier 3.5)";
                    }
                }
            }

            if let Some(success_content) = new_content {
                current_content = success_content;
                println!(
                    "✨ [SUCCESS] Applied block {} to {} using strategy: {}",
                    idx + 1,
                    raw_path,
                    strategy
                );
            } else {
                eprintln!(
                    "FATAL ERROR in file: {}\nBlock {} failed to match. Target snippet could not be matched safely.",
                    raw_path,
                    idx + 1
                );
                let closest_context = find_closest_match_context(&current_content, search_part);
                eprintln!("SEARCH BLOCK:\n{}", search_part);
                eprintln!("REPLACEMENT:\n{}", replacement);
                eprintln!(
                    "CLOSEST ACTUAL REPOSITORY CONTEXT SNIPPET:\n{}",
                    closest_context
                );
                errors_found = true;
                break;
            }
        }

        file_updates.insert(file_path, current_content);
    }

    if errors_found {
        eprintln!("🛑 Transaction aborted. No files were modified on disk.");
        std::process::exit(1);
    }

    // Write all verified modifications to disk transactionally
    for (file_path, new_text) in &file_updates {
        if let Some(parent) = file_path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                eprintln!(
                    "FATAL ERROR: Failed to create directories for path {:?}: {}",
                    file_path, e
                );
                std::process::exit(1);
            }
        }
        if let Err(e) = fs::write(file_path, new_text) {
            eprintln!("FATAL ERROR: Failed to write file {:?}: {}", file_path, e);
            std::process::exit(1);
        }
        println!("✅ {:?} updated successfully.", file_path);
    }

    println!("\nDone. All operations completed successfully.");
}

// ==============================================================================
// Unit Test Suite
// ==============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    // --- Helper function tests ---
    #[test]
    fn test_prep_adds_missing_newline() {
        let (adjusted, lines) = prep("hello\nworld");
        assert_eq!(adjusted, "hello\nworld\n");
        assert_eq!(lines, vec!["hello\n".to_string(), "world\n".to_string()]);
    }

    #[test]
    fn test_prep_preserves_existing_newline() {
        let (adjusted, lines) = prep("hello\nworld\n");
        assert_eq!(adjusted, "hello\nworld\n");
        assert_eq!(lines, vec!["hello\n".to_string(), "world\n".to_string()]);
    }

    // --- Tier 1 Match Tests ---
    #[test]
    fn test_perfect_replace_exact_match() {
        let whole = vec![
            "line_1\n".to_string(),
            "target_line\n".to_string(),
            "line_3\n".to_string(),
        ];
        let part = vec!["target_line\n".to_string()];
        let replace = vec![
            "replacement_line_a\n".to_string(),
            "replacement_line_b\n".to_string(),
        ];

        let result = perfect_replace(&whole, &part, &replace);
        assert_eq!(
            result,
            Some("line_1\nreplacement_line_a\nreplacement_line_b\nline_3\n".to_string())
        );
    }

    #[test]
    fn test_perfect_replace_fails_when_part_missing() {
        let whole = vec!["line_1\n".to_string(), "line_2\n".to_string()];
        let part = vec!["target_not_present\n".to_string()];
        let replace = vec!["replacement\n".to_string()];

        let result = perfect_replace(&whole, &part, &replace);
        assert_eq!(result, None);
    }

    // --- Tier 2 Indentation-adjusted Match Tests ---
    #[test]
    fn test_replace_part_with_missing_leading_whitespace() {
        let whole = vec![
            "    class Temp {\n".to_string(),
            "        fn inner() {}\n".to_string(),
            "    }\n".to_string(),
        ];
        let part = vec!["fn inner() {}\n".to_string()];
        let replace = vec!["fn adjusted_inner() {}\n".to_string()];

        let result = replace_part_with_missing_leading_whitespace(&whole, &part, &replace);
        assert_eq!(
            result,
            Some("    class Temp {\n        fn adjusted_inner() {}\n    }\n".to_string())
        );
    }

    // --- Tier 2.5 Elision Match Tests ---
    #[test]
    fn test_try_dotdotdots_success() {
        let whole = "fn test() {\n    let x = 1;\n    let y = 2;\n    println!(\"{}\", x);\n}";
        let part = "fn test() {\n...\n    println!(\"{}\", x);\n}";
        let replace = "fn test() {\n...\n    println!(\"patched\");\n}";

        let result = try_dotdotdots(whole, part, replace);
        assert!(result.is_some());
        assert_eq!(
            result.unwrap(),
            "fn test() {\n    let x = 1;\n    let y = 2;\n    println!(\"patched\");\n}"
        );
    }

    #[test]
    fn test_try_dotdotdots_uniqueness_check() {
        let whole = "let x = 1;\nlet y = 2;\nlet x = 1;\nlet z = 3;";
        let part = "let x = 1;\n...\nlet z = 3;";
        let replace = "let mutated = 1;\n...\nlet z = 3;";

        let result = try_dotdotdots(whole, part, replace);
        assert_eq!(result, None);
    }

    // --- Tier 3 Fuzzy Sequence Match Tests ---
    #[test]
    fn test_replace_closest_edit_distance() {
        let whole_lines = vec![
            "fn evaluate_model() {\n".to_string(),
            "    let a = 100;\n".to_string(),
            "    let b = 200;\n".to_string(),
            "    let c = 300;\n".to_string(),
            "    let d = 400;\n".to_string(),
            "    let e = 500;\n".to_string(),
            "    let f = 600;\n".to_string(),
            "    let g = 700;\n".to_string(),
            "    let h = 800;\n".to_string(),
            "}\n".to_string(),
        ];
        let part = "fn evaluate_model() {\n    let a = 100;\n    let b = 200;\n    let c = 300;\n    let d = 400;\n    let e = 505;\n    let f = 600;\n    let g = 700;\n    let h = 800;\n}\n";
        let part_lines = vec![
            "fn evaluate_model() {\n".to_string(),
            "    let a = 100;\n".to_string(),
            "    let b = 200;\n".to_string(),
            "    let c = 300;\n".to_string(),
            "    let d = 400;\n".to_string(),
            "    let e = 505;\n".to_string(),
            "    let f = 600;\n".to_string(),
            "    let g = 700;\n".to_string(),
            "    let h = 800;\n".to_string(),
            "}\n".to_string(),
        ];
        let replace_lines = vec![
            "fn evaluate_model() {\n".to_string(),
            "    let a = 1000;\n".to_string(),
            "}\n".to_string(),
        ];

        let result = replace_closest_edit_distance(&whole_lines, part, &part_lines, &replace_lines);
        assert!(result.is_some());
        assert_eq!(
            result.unwrap(),
            "fn evaluate_model() {\n    let a = 1000;\n}\n"
        );
    }

    // --- SEARCH/REPLACE Parser Tests ---
    #[test]
    fn test_parse_diff_blocks_multiple() {
        let diff_payload = r#"
Some descriptive text before the block.

<<<<<<< SEARCH
old_func_1();
=======
new_func_1();
>>>>>>> REPLACE

Middle text...

<<<<<<< SEARCH
old_func_2();
=======
new_func_2();
>>>>>>> REPLACE
"#;
        let blocks = parse_diff_blocks(diff_payload);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].0, "old_func_1();");
        assert_eq!(blocks[0].1, "new_func_1();");
        assert_eq!(blocks[1].0, "old_func_2();");
        assert_eq!(blocks[1].1, "new_func_2();");
    }

    // --- Tree-Sitter / AST Logic Tests ---
    #[test]
    fn test_tree_sitter_rust_ast_declarations() {
        let mut parser = TsParser::new();
        let rust_language = tree_sitter_rust::LANGUAGE;
        parser.set_language(&rust_language.into()).unwrap();

        let src = r#"
            struct Config {
                port: u16,
            }

            impl Config {
                fn init() -> Self {
                    Config { port: 8080 }
                }
            }

            fn execute_task() {}
        "#;

        let tree = parser.parse(src, None).unwrap();
        let entities = find_declared_entities(tree.root_node(), src.as_bytes(), true);

        let has_struct = entities
            .iter()
            .any(|(kind, name)| kind == "struct_item" && name == "Config");
        let has_impl = entities
            .iter()
            .any(|(kind, name)| kind == "impl_item" && name == "Config");
        let has_fn = entities.iter().any(|(kind, name)| {
            kind == "function_item" && name == "init" || name == "execute_task"
        });

        assert!(has_struct, "Failed to resolve struct_item: 'Config'");
        assert!(has_impl, "Failed to resolve impl_item: 'Config'");
        assert!(has_fn, "Failed to resolve function_item signatures");
    }

    #[test]
    fn test_tree_sitter_tsx_ast_declarations() {
        let mut parser = TsParser::new();
        let tsx_language = tree_sitter_typescript::LANGUAGE_TSX;
        parser.set_language(&tsx_language.into()).unwrap();

        let src = r#"
            class UserProfile extends React.Component {
                render() {
                    return <div>Profile</div>;
                }
            }

            function getUserID() {}
        "#;

        let tree = parser.parse(src, None).unwrap();
        let entities = find_declared_entities(tree.root_node(), src.as_bytes(), false);

        let has_class = entities
            .iter()
            .any(|(kind, name)| kind == "class_declaration" && name == "UserProfile");
        let has_method = entities
            .iter()
            .any(|(kind, name)| kind == "method_definition" && name == "render");
        let has_fn = entities
            .iter()
            .any(|(kind, name)| kind == "function_declaration" && name == "getUserID");

        assert!(has_class, "Failed to find class_declaration: 'UserProfile'");
        assert!(has_method, "Failed to find method_definition: 'render'");
        assert!(has_fn, "Failed to find function_declaration: 'getUserID'");
    }

    // --- AST Fallback (Tier 3.5) Integration Tests ---
    #[test]
    fn test_ts_ast_fallback_success() {
        let mut parser = TsParser::new();
        let tsx_language = tree_sitter_typescript::LANGUAGE_TSX;
        parser.set_language(&tsx_language.into()).unwrap();

        let target = "class ConnectionManager {\n    public connect() {\n        console.log(\"attempting old\");\n    }\n}";
        let search = "   public connect() {\n        console.log(\"attempting old\");\n    }";
        let replacement = "    public connect() {\n        console.log(\"attempting new\");\n    }";

        let result = replace_via_ast_fallback(&mut parser, target, search, replacement, false);
        assert!(result.is_some());

        let updated = result.unwrap();
        assert!(updated.contains("attempting new"));
        assert!(!updated.contains("attempting old"));
    }

    #[test]
    fn test_rust_ast_fallback_success() {
        let mut parser = TsParser::new();
        let rust_language = tree_sitter_rust::LANGUAGE;
        parser.set_language(&rust_language.into()).unwrap();

        let target = "impl State {\n    fn transition(&mut self) {\n        self.step = Step::First;\n    }\n}";
        let search = "fn transition  ( &mut self ) {\n        self.step = Step::First;\n    }";
        let replacement =
            "    fn transition(&mut self) {\n        self.step = Step::Second;\n    }";

        let result = replace_via_ast_fallback(&mut parser, target, search, replacement, true);
        assert!(result.is_some());

        let updated = result.unwrap();
        assert!(updated.contains("self.step = Step::Second;"));
        assert!(!updated.contains("self.step = Step::First;"));
    }
}
