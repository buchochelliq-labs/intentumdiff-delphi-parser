//! Delphi / Object Pascal parser plugin — interpret-cst mode.
//!
//! Handles `.pas`, `.dpr`, and `.dfm` files.
//! The host parses source with tree-sitter-pascal and sends the CST as JSON.

use intentumdiff_plugin_sdk::tree::{SemanticNode, SemanticNodeBuilder};

wit_bindgen::generate!({
    path: "wit/plugin.wit",
    world: "parser-plugin",
});

use crate::exports::intentdiff::plugin::parser::ExamplePair;
use crate::exports::intentdiff::plugin::parser::Guest;
use crate::exports::intentdiff::plugin::parser::LanguageInfoRecord;
use crate::exports::intentdiff::plugin::parser::ParserMode;

const PLUGIN_METADATA: &str = include_str!("../plugin_metadata.info");

fn language_info_for(ids: Vec<String>) -> Vec<LanguageInfoRecord> {
    let metadata = intentumdiff_plugin_sdk::metadata::parse_plugin_metadata(PLUGIN_METADATA);
    ids.into_iter()
        .map(|language_id| {
            let info = metadata.language_or_default(&language_id);
            LanguageInfoRecord {
                language_id: info.language_id,
                language_name: info.language_name,
                language_short_name: info.language_short_name,
                monaco_language: info.monaco_language,
                default_filename: info.default_filename,
                language_file_extensions: info.language_file_extensions,
                author: metadata.author().to_string(),
                plugin_version: metadata.plugin_version().to_string(),
                last_updated: metadata.last_updated().to_string(),
            }
        })
        .collect()
}
struct DelphiParser;

const TRIVIA: &[&str] = &[
    "comment",
    "single_line_comment",
    "multi_line_comment",
    "whitespace",
];

const SEMANTIC_TYPES: &[&str] = &[
    // Root
    "source_file",
    "program",
    "unit",
    "moduleName",
    "unit_declaration",
    "library",
    "package_decl",
    // Type declarations
    "class_type",
    "class_definition",
    "record_type",
    "record_definition",
    "interface_type",
    "interface_definition",
    "object_type",
    "type_declaration",
    "type_def",
    // Routine declarations
    "function_declaration",
    "function_definition",
    "function_heading",
    "procedure_declaration",
    "procedure_definition",
    "procedure_heading",
    "constructor_declaration",
    "constructor_definition",
    "destructor_declaration",
    "destructor_definition",
    "method_declaration",
    "method_definition",
    "defProc",
    "declProc",
    "declArgs",
    "declArg",
    // Properties
    "property_declaration",
    "property_definition",
    // Sections
    "var_block",
    "var_section",
    "const_block",
    "const_section",
    "type_block",
    "type_section",
    "var_declaration",
    "const_declaration",
    // Imports
    "uses_clause",
    "uses_list",
    // Control flow
    "if_statement",
    "for_statement",
    "for_to_statement",
    "for_downto_statement",
    "for_in_statement",
    "while_statement",
    "repeat_statement",
    "case_statement",
    "case_item",
    "with_statement",
    "try_statement",
    "try_except_statement",
    "try_finally_statement",
    "except_clause",
    "finally_clause",
    "raise_statement",
    "exit_statement",
    "break_statement",
    "continue_statement",
    // Statements
    "compound_statement",
    "begin_end",
    "block",
    "statements",
    "statement",
    "assignment_statement",
    "assignment",
    "procedure_call",
    "exprCall",
    "exprBinary",
    "literalString",
    "literalNumber",
    "inherited_call",
];

fn is_semantic(node_type: &str) -> bool {
    SEMANTIC_TYPES.contains(&node_type)
}

fn is_class_like(node_type: &str) -> bool {
    matches!(
        node_type,
        "class_type"
            | "class_definition"
            | "record_type"
            | "record_definition"
            | "interface_type"
            | "interface_definition"
            | "object_type"
            | "unit_declaration"
            | "unit"
            | "program"
            | "moduleName"
    )
}

fn is_method_like(node_type: &str) -> bool {
    matches!(
        node_type,
        "function_declaration"
            | "function_definition"
            | "function_heading"
            | "procedure_declaration"
            | "procedure_definition"
            | "procedure_heading"
            | "constructor_declaration"
            | "constructor_definition"
            | "destructor_declaration"
            | "destructor_definition"
            | "method_declaration"
            | "method_definition"
            | "defProc"
            | "declProc"
    )
}

fn label_for_ts(node: tree_sitter::Node<'_>, source: &[u8]) -> String {
    let kind = node.kind();
    let txt = |n: tree_sitter::Node<'_>| n.utf8_text(source).unwrap_or("").to_string();
    if node.child_count() == 0 {
        return node.utf8_text(source).unwrap_or("").to_string();
    }
    match kind {
        "unit_declaration" | "unit" | "program" | "library" | "package_decl" => {
            for i in 0..node.child_count() {
                let c = node.child(i).unwrap();
                if matches!(c.kind(), "identifier" | "unit_name" | "name" | "moduleName") {
                    return txt(c);
                }
            }
        }
        "defProc" => {
            if let Some(header) = node.child_by_field_name("header") {
                return label_for_ts(header, source);
            }
        }
        "class_type"
        | "class_definition"
        | "record_type"
        | "record_definition"
        | "interface_type"
        | "interface_definition"
        | "object_type"
        | "type_declaration"
        | "type_def" => {
            for i in 0..node.child_count() {
                let c = node.child(i).unwrap();
                if matches!(c.kind(), "identifier" | "type_name") {
                    return txt(c);
                }
            }
        }
        "function_declaration"
        | "function_definition"
        | "function_heading"
        | "procedure_declaration"
        | "procedure_definition"
        | "procedure_heading"
        | "constructor_declaration"
        | "constructor_definition"
        | "destructor_declaration"
        | "destructor_definition"
        | "method_declaration"
        | "method_definition" => {
            if let Some(name) = node.child_by_field_name("name") {
                return txt(name);
            }
            let mut parts: Vec<String> = Vec::new();
            for i in 0..node.child_count() {
                let c = node.child(i).unwrap();
                if c.kind() == "identifier" || c.kind() == "qualified_identifier" {
                    parts.push(txt(c));
                    if parts.len() >= 2 {
                        break;
                    }
                }
            }
            if !parts.is_empty() {
                return parts.join(".");
            }
        }
        "declProc" | "declArg" => {
            if let Some(name) = node.child_by_field_name("name") {
                return txt(name);
            }
        }
        "assignment" => {
            if let Some(lhs) = node.child_by_field_name("lhs") {
                return txt(lhs);
            }
        }
        "exprCall" => {
            if let Some(entity) = node.child_by_field_name("entity") {
                return txt(entity);
            }
        }
        "property_declaration"
        | "property_definition"
        | "var_declaration"
        | "const_declaration" => {
            for i in 0..node.child_count() {
                let c = node.child(i).unwrap();
                if c.kind() == "identifier" {
                    return txt(c);
                }
            }
        }
        "procedure_call" | "inherited_call" => {
            for i in 0..node.child_count() {
                let c = node.child(i).unwrap();
                if matches!(
                    c.kind(),
                    "identifier" | "qualified_identifier" | "method_name"
                ) {
                    return txt(c);
                }
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() {
        let c = node.child(i).unwrap();
        if c.kind() == "identifier" {
            return txt(c);
        }
    }
    kind.to_string()
}

fn convert_ts(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    id_prefix: &str,
    parent_class: Option<&str>,
) -> Option<SemanticNode> {
    if TRIVIA.contains(&node.kind()) {
        return None;
    }

    let child_parent_class = if is_class_like(node.kind()) {
        Some(label_for_ts(node, source))
    } else {
        parent_class.map(|s| s.to_string())
    };

    let children: Vec<SemanticNode> = (0..node.child_count())
        .filter_map(|i| {
            convert_ts(
                node.child(i)?,
                source,
                &format!("{}.{}", id_prefix, i),
                child_parent_class.as_deref(),
            )
        })
        .collect();

    if !is_semantic(node.kind()) && children.is_empty() {
        return None;
    }

    let mut builder = SemanticNodeBuilder::new(
        id_prefix,
        node.kind(),
        label_for_ts(node, source),
        node.start_position().row as u32,
        node.start_position().column as u32,
        node.end_position().row as u32,
        node.end_position().column as u32,
        "",
    )
    .children(children);

    if is_method_like(node.kind()) {
        if let Some(class_name) = parent_class {
            builder = builder.parent_type(class_name);
        }
    }

    Some(builder.build())
}

fn process_impl(source: &str) -> String {
    let mut parser = tree_sitter::Parser::new();
    let lang = tree_sitter_pascal::LANGUAGE.into();
    if parser.set_language(&lang).is_err() {
        return r#"{"error":"Failed to load Delphi grammar"}"#.to_string();
    }
    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => return r#"{"error":"Parse failed"}"#.to_string(),
    };
    let root = tree.root_node();
    match convert_ts(root, source.as_bytes(), "0", None) {
        Some(n) => serde_json::to_string(&n).unwrap_or_else(|e| format!(r#"{{"error":"{}"}}"#, e)),
        None => r#"{"error":"Empty semantic tree"}"#.to_string(),
    }
}
impl Guest for DelphiParser {
    fn get_parser_mode() -> ParserMode {
        ParserMode::FullParse
    }
    fn grammar_id() -> String {
        "delphi".to_string()
    }
    fn detect_language(filename: String, _content: String) -> String {
        let lower = filename.to_lowercase();
        if lower.ends_with(".pas")
            || lower.ends_with(".dpr")
            || lower.ends_with(".dfm")
            || lower.ends_with(".dpk")
        {
            return "delphi".to_string();
        }
        String::new()
    }
    fn preprocess_source(source: String) -> String {
        source
    }
    fn process(input: String, _language: String, _filename: String) -> String {
        process_impl(&input)
    }
    fn trivia_node_types() -> Vec<String> {
        TRIVIA.iter().map(|s| s.to_string()).collect()
    }
    fn language_ids() -> Vec<String> {
        vec!["delphi".to_string()]
    }
    fn language_info() -> Vec<LanguageInfoRecord> {
        language_info_for(Self::language_ids())
    }
    fn priority() -> i32 {
        0
    }

    fn example(_language: String) -> ExamplePair {
        ExamplePair {
            old: "program Demo;\n\nprocedure Greet(const Name: string);\nbegin\n  WriteLn('Hello, ' + Name);\nend;\n\nfunction Add(A, B: Integer): Integer;\nbegin\n  Result := A + B;\nend;\n\nbegin\n  Greet('World');\n  WriteLn(Add(2, 3));\nend.\n".to_string(),
            new: "program Demo;\n\nprocedure Greet(const Name: string);\nbegin\n  WriteLn(Format('Hello, %s!', [Name]));\nend;\n\nfunction Add(const A, B: Integer): Integer;\nbegin\n  Result := A + B;\nend;\n\nfunction Multiply(const A, B: Integer): Integer;\nbegin\n  Result := A * B;\nend;\n\nbegin\n  Greet('World');\n  WriteLn(Add(2, 3));\n  WriteLn(Multiply(2, 3));\nend.\n".to_string(),
        }
    }
}
export!(DelphiParser);

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exports::intentdiff::plugin::parser::Guest;
    use intentumdiff_plugin_sdk::testing as t;

    #[test]
    fn grammar_id_nonempty() {
        assert!(!DelphiParser::grammar_id().is_empty());
    }

    #[test]
    fn language_ids_contain_grammar_id() {
        let gid = DelphiParser::grammar_id();
        let ids = DelphiParser::language_ids();
        assert!(
            ids.contains(&gid),
            "language_ids {:?} must contain {:?}",
            ids,
            gid
        );
    }

    #[test]
    fn detect_language_known_ext() {
        let r = DelphiParser::detect_language("test.pas".to_string(), "".to_string());
        assert_eq!(r.as_str(), "delphi");
    }

    #[test]
    fn detect_language_unknown_ext() {
        let r =
            DelphiParser::detect_language("test.xyz_notareal_ext_9z8y".to_string(), "".to_string());
        assert_eq!(r.as_str(), "");
    }

    #[test]
    fn process_impl_empty_returns_valid_json() {
        let out = process_impl("");
        t::assert_valid_json(&out, "process(empty)");
    }

    #[test]
    fn process_impl_whitespace_returns_valid_json() {
        let out = process_impl("   \n  ");
        t::assert_valid_json(&out, "process(whitespace)");
    }

    #[test]
    fn playground_example_produces_semantic_nodes() {
        let example = <DelphiParser as Guest>::example("delphi".to_string());
        let out = process_impl(&example.new);
        t::assert_valid_json(&out, "delphi example");
        t::assert_no_error(&out, "delphi example");
        t::assert_contains_node_type(&out, "defProc", "delphi example");
        assert!(
            out.contains("Multiply"),
            "expected Delphi example to include added function: {}",
            out
        );
    }
}
