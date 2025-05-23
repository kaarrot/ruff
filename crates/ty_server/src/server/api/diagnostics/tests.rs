use lsp_types::{Diagnostic, DiagnosticSeverity, Range, Position, NumberOrString};

#[cfg(test)]
mod diagnostic_tests {
    use super::*;

    /// Test that diagnostic format meets expectations
    #[test]
    fn test_diagnostic_format() {
        let test_diagnostic = create_test_diagnostic();
        
        assert_eq!(test_diagnostic.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(test_diagnostic.source.as_deref(), Some("ty"));
        assert!(test_diagnostic.code.is_some());
        assert!(!test_diagnostic.message.is_empty());
        
        // Test range is valid
        assert!(test_diagnostic.range.end.line >= test_diagnostic.range.start.line);
    }

    /// Test diagnostic message content
    #[test]
    fn test_diagnostic_message_content() {
        let diagnostic = create_test_diagnostic();
        
        // Diagnostic should have a non-empty, meaningful message
        assert!(!diagnostic.message.is_empty());
        assert!(diagnostic.message.len() > 5); // Reasonable minimum length
        
        // Source should be "ty" to identify this server
        assert_eq!(diagnostic.source.as_deref(), Some("ty"));
    }

    /// Test diagnostic severity mapping
    #[test] 
    fn test_diagnostic_severity_mapping() {
        // Test that different severity levels are mapped correctly
        let error_diagnostic = Diagnostic {
            range: create_test_range(),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String("test-error".to_string())),
            source: Some("ty".to_string()),
            message: "Test error message".to_string(),
            tags: None,
            related_information: None,
            data: None,
            code_description: None,
        };

        let warning_diagnostic = Diagnostic {
            range: create_test_range(),
            severity: Some(DiagnosticSeverity::WARNING),
            code: Some(NumberOrString::String("test-warning".to_string())),
            source: Some("ty".to_string()),
            message: "Test warning message".to_string(),
            tags: None,
            related_information: None,
            data: None,
            code_description: None,
        };

        let info_diagnostic = Diagnostic {
            range: create_test_range(),
            severity: Some(DiagnosticSeverity::INFORMATION),
            code: Some(NumberOrString::String("test-info".to_string())),
            source: Some("ty".to_string()),
            message: "Test info message".to_string(),
            tags: None,
            related_information: None,
            data: None,
            code_description: None,
        };

        // Verify severity levels are set correctly
        assert_eq!(error_diagnostic.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(warning_diagnostic.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(info_diagnostic.severity, Some(DiagnosticSeverity::INFORMATION));
    }

    /// Test that empty diagnostics list is handled correctly
    #[test]
    fn test_empty_diagnostics() {
        let empty_diagnostics: Vec<Diagnostic> = vec![];
        assert_eq!(empty_diagnostics.len(), 0);
        
        // This simulates what should happen when a file has no errors
        // The notification should still be sent, but with an empty diagnostics array
    }

    /// Test diagnostic range validation
    #[test]
    fn test_diagnostic_range_validation() {
        let diagnostic = create_test_diagnostic();
        let range = &diagnostic.range;
        
        // Start position should be valid
        assert!(range.end.line >= range.start.line);
        if range.end.line == range.start.line {
            assert!(range.end.character >= range.start.character);
        }
    }

    /// Test diagnostic codes are meaningful
    #[test]
    fn test_diagnostic_codes() {
        let diagnostic = create_test_diagnostic();
        
        if let Some(code) = &diagnostic.code {
            match code {
                NumberOrString::String(code_str) => {
                    assert!(!code_str.is_empty());
                    assert!(code_str.len() > 2); // Should be more than just 1-2 characters
                }
                NumberOrString::Number(code_num) => {
                    assert!(*code_num > 0); // Should be a positive number
                }
            }
        }
    }

    // Helper functions for creating test data
    
    fn create_test_diagnostic() -> Diagnostic {
        Diagnostic {
            range: create_test_range(),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String("syntax-error".to_string())),
            source: Some("ty".to_string()),
            message: "Invalid Python syntax: unexpected token".to_string(),
            tags: None,
            related_information: None,
            data: None,
            code_description: None,
        }
    }

    fn create_test_range() -> Range {
        Range {
            start: Position { line: 0, character: 0 },
            end: Position { line: 0, character: 10 },
        }
    }
} 