//! Korg Code Intelligence Layer (`src/code_intel.rs`)
//!
//! Provides advanced syntax-aware code parsing, AST symbol extraction, S-expression
//! structural search, and pre-flight error-tolerant syntax validation using Tree-sitter.

use std::path::Path;
use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator};

/// Supported programming languages in Korg
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KorgLanguage {
    Rust,
    Python,
}

impl KorgLanguage {
    /// Detects language from a file path extension
    pub fn from_path<P: AsRef<Path>>(path: P) -> Option<Self> {
        match path.as_ref().extension()?.to_str()? {
            "rs" => Some(Self::Rust),
            "py" => Some(Self::Python),
            _ => None,
        }
    }

    /// Resolves the corresponding Tree-sitter language grammar
    pub fn tree_sitter_lang(&self) -> Language {
        match self {
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
        }
    }

    /// String identifier of the language
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Python => "python",
        }
    }
}

/// Represents a syntax anomaly caught during pre-flight validation
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct SyntaxAnomaly {
    pub line: usize,   // 1-indexed
    pub column: usize, // 1-indexed
    pub severity: String,
    pub kind: String,
    pub context: String,
}

/// Structural symbol map representation for agent context injections
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct CodeSymbol {
    pub name: String,
    pub kind: String,
    pub start_line: usize, // 1-indexed
    pub end_line: usize,   // 1-indexed
}

/// Dynamic match coordinate for S-expression queries
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct StructuralMatch {
    pub matched_text: String,
    pub capture_name: String,
    pub start_line: usize,
    pub end_line: usize,
}

/// Core syntax-aware parser engine
pub struct CodeIntelEngine;

impl CodeIntelEngine {
    /// Performs an error-tolerant AST parse and extracts any syntax errors or missing structures.
    pub fn validate_syntax(source: &str, lang: KorgLanguage) -> Vec<SyntaxAnomaly> {
        let mut parser = Parser::new();
        if parser.set_language(&lang.tree_sitter_lang()).is_err() {
            return vec![];
        }

        let tree = match parser.parse(source, None) {
            Some(t) => t,
            None => return vec![],
        };

        let mut anomalies = Vec::new();
        Self::traverse_anomalies(tree.root_node(), source, &mut anomalies);
        anomalies
    }

    /// Recursively scans AST nodes searching for tree-sitter error/missing tokens.
    fn traverse_anomalies(node: tree_sitter::Node, source: &str, anomalies: &mut Vec<SyntaxAnomaly>) {
        if node.is_error() {
            let start = node.start_position();
            let context = node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
            anomalies.push(SyntaxAnomaly {
                line: start.row + 1,
                column: start.column + 1,
                severity: "Error".to_string(),
                kind: "SyntaxError".to_string(),
                context: if context.is_empty() { "Unexpected token or syntax structure".to_string() } else { context },
            });
        } else if node.is_missing() {
            let start = node.start_position();
            anomalies.push(SyntaxAnomaly {
                line: start.row + 1,
                column: start.column + 1,
                severity: "Warning".to_string(),
                kind: "MissingToken".to_string(),
                context: format!("Missing required syntax element: {}", node.kind()),
            });
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::traverse_anomalies(child, source, anomalies);
        }
    }

    /// AST Symbol Extractor
    ///
    /// Resolves high-density symbol layouts to give agents quick files overview.
    pub fn extract_symbols(source: &str, lang: KorgLanguage) -> Vec<CodeSymbol> {
        let mut parser = Parser::new();
        let ts_lang = lang.tree_sitter_lang();
        if parser.set_language(&ts_lang).is_err() {
            return vec![];
        }

        let tree = match parser.parse(source, None) {
            Some(t) => t,
            None => return vec![],
        };

        let query_str = match lang {
            KorgLanguage::Rust => r#"
                (function_item name: (identifier) @name) @item
                (struct_item name: (type_identifier) @name) @item
                (impl_item type: (type_identifier) @name) @item
                (trait_item name: (type_identifier) @name) @item
                (mod_item name: (identifier) @name) @item
            "#,
            KorgLanguage::Python => r#"
                (function_definition name: (identifier) @name) @item
                (class_definition name: (identifier) @name) @item
            "#,
        };

        let query = match Query::new(&ts_lang, query_str) {
            Ok(q) => q,
            Err(_) => return vec![],
        };

        let mut cursor = QueryCursor::new();
        let mut symbols = Vec::new();

        let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
        while let Some(m) = matches.next() {
            let mut matched_name = None;
            let mut matched_kind = "unknown";
            let mut start_line = 0;
            let mut end_line = 0;

            for cap in m.captures {
                let capture_name = query.capture_names()[cap.index as usize];
                if capture_name == "name" {
                    if let Ok(name_text) = cap.node.utf8_text(source.as_bytes()) {
                        matched_name = Some(name_text.to_string());
                    }
                } else if capture_name == "item" {
                    matched_kind = cap.node.kind();
                    start_line = cap.node.start_position().row + 1;
                    end_line = cap.node.end_position().row + 1;
                }
            }

            if let Some(name) = matched_name {
                symbols.push(CodeSymbol {
                    name,
                    kind: matched_kind.to_string(),
                    start_line,
                    end_line,
                });
            }
        }

        // Deduplicate and sort by appearance order
        symbols.sort_by_key(|s| s.start_line);
        symbols.dedup();
        symbols
    }

    /// Structural Search matching Lisp-style S-expressions
    pub fn query_structure(source: &str, lang: KorgLanguage, query_str: &str) -> Result<Vec<StructuralMatch>, String> {
        let mut parser = Parser::new();
        let ts_lang = lang.tree_sitter_lang();
        if parser.set_language(&ts_lang).is_err() {
            return Err("Failed to configure tree-sitter language parser".to_string());
        }

        let tree = parser.parse(source, None)
            .ok_or_else(|| "Failed to parse code tree".to_string())?;

        let query = Query::new(&ts_lang, query_str)
            .map_err(|e| format!("Invalid S-expression query pattern: {}", e))?;

        let mut cursor = QueryCursor::new();
        let mut matches_out = Vec::new();

        let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
        while let Some(m) = matches.next() {
            for cap in m.captures {
                let capture_name = query.capture_names()[cap.index as usize].to_string();
                let matched_text = cap.node.utf8_text(source.as_bytes())
                    .unwrap_or("")
                    .to_string();
                let start_line = cap.node.start_position().row + 1;
                let end_line = cap.node.end_position().row + 1;

                matches_out.push(StructuralMatch {
                    matched_text,
                    capture_name,
                    start_line,
                    end_line,
                });
            }
        }

        Ok(matches_out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_syntax_validation_catches_errors() {
        let broken_rust = r#"
            fn calculate_auth(token: &str) {
                let mut hasher = Sha256::new()
                // Syntax Error: Missing closing parenthesis/braces and semicolons
                if token.is_empty() {
                    return
        "#;

        let anomalies = CodeIntelEngine::validate_syntax(broken_rust, KorgLanguage::Rust);
        assert!(!anomalies.is_empty());
        let has_syntax_error = anomalies.iter().any(|a| a.kind == "SyntaxError");
        assert!(has_syntax_error);
    }

    #[test]
    fn test_symbol_extraction_precision() {
        let sample_rust = r#"
            mod auth_gate {
                pub struct UserSession {
                    pub token: String,
                }

                impl UserSession {
                    pub fn is_valid(&self) -> bool {
                        true
                    }
                }
            }
        "#;

        let symbols = CodeIntelEngine::extract_symbols(sample_rust, KorgLanguage::Rust);
        assert!(!symbols.is_empty());

        let has_struct = symbols.iter().any(|s| s.name == "UserSession" && s.kind == "struct_item");
        let has_impl = symbols.iter().any(|s| s.name == "UserSession" && s.kind == "impl_item");
        let has_fn = symbols.iter().any(|s| s.name == "is_valid" && s.kind == "function_item");

        assert!(has_struct);
        assert!(has_impl);
        assert!(has_fn);
    }

    #[test]
    fn test_ast_s_expression_query() {
        let sample_python = r#"
def resolve_campaign(tx_id):
    if tx_id is None:
        return "aborted"
    return "completed"
        "#;

        let query = "(function_definition name: (identifier) @func.name)";
        let matches = CodeIntelEngine::query_structure(sample_python, KorgLanguage::Python, query).unwrap();
        assert!(!matches.is_empty());
        assert_eq!(matches[0].capture_name, "func.name");
        assert_eq!(matches[0].matched_text, "resolve_campaign");
    }
}
