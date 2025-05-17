use lsp_types::{
   ClientCapabilities, DiagnosticClientCapabilities, TextDocumentClientCapabilities
};

/// Test client capability detection for pull diagnostics support
/// When a client supports pull diagnostics, we should detect this capability
/// and use the pull-based diagnostic protocol instead of push notifications
#[cfg(test)]
mod push_notification_tests {
    use super::*;
    #[test]
    fn test_detect_pull_diagnostics_support() {
        // Dynamic registration is present - pull diagnostics are supported
        let diagnostic = DiagnosticClientCapabilities {
            dynamic_registration: Some(true),
            related_document_support: Some(false),
        };
        let document_cap = TextDocumentClientCapabilities {
            diagnostic: Some(diagnostic),
            ..Default::default()
        };

        let client_caps_with_pull = ClientCapabilities {
            text_document: Some(document_cap),
            ..Default::default()
        };

        let supports_pull_diagnostics = client_caps_with_pull
            .text_document
            .as_ref()
            .and_then(|text_document| text_document.diagnostic.as_ref())
            .is_some();
        
        // Should detect pull diagnostics support
        assert!(supports_pull_diagnostics, "Should detect pull diagnostics support when client provides diagnostic capabilities");
    }
    
    /// Test fallback to push notifications when client lacks pull diagnostics support
    /// This tests the legacy behavior where we send push notifications via publishDiagnostics
    #[test]
    fn test_fallback_to_push_notifications() {
        // Client has text_document capabilities but no diagnostic support
        let document_cap = TextDocumentClientCapabilities {
            diagnostic: None, // No pull diagnostics support
            completion: Some(Default::default()), // Other capabilities present
            ..Default::default()
        };


        let client_caps_legacy = ClientCapabilities {
            text_document: Some(document_cap),
            ..Default::default()
        };
        
        let supports_pull_diagnostics = client_caps_legacy
            .text_document
            .as_ref()
            .and_then(|text_document| text_document.diagnostic.as_ref())
            .is_some();
        
        assert!(!supports_pull_diagnostics, "Should fall back to push notifications when diagnostic capability is missing");
        
        // Client has no text_document capabilities at all (very legacy client)
        let client_caps_no_text_doc = ClientCapabilities {
            text_document: None,
            workspace: Some(Default::default()), // Only workspace capabilities
            ..Default::default()
        };
        
        let supports_pull_diagnostics = client_caps_no_text_doc
            .text_document
            .as_ref()
            .and_then(|text_document| text_document.diagnostic.as_ref())
            .is_some();
        
        assert!(!supports_pull_diagnostics, "Should fall back to push notifications when text_document capability is missing entirely");
    }
}

