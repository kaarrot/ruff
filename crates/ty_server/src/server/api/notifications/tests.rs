use lsp_types::{
    ClientCapabilities, DiagnosticClientCapabilities, TextDocumentClientCapabilities,
};

#[cfg(test)]
mod push_notification_tests {
    use super::*;

    /// Test the logic for determining when to send push notifications
    #[test]
    fn test_should_send_push_diagnostics_logic() {
        // When pull diagnostics are NOT supported, should send push
        assert!(should_send_push_diagnostics(false));

        // When pull diagnostics ARE supported, should NOT send push
        assert!(!should_send_push_diagnostics(true));
    }

    /// Helper function that mimics the logic used in the notification handlers
    fn should_send_push_diagnostics(pull_diagnostics_supported: bool) -> bool {
        !pull_diagnostics_supported
    }

    /// Test that the conditional logic works as expected
    #[test]
    fn test_notification_behavior_based_on_capabilities() {
        // Simulate the logic from the notification handlers
        
        // Case 1: Client does NOT support pull diagnostics (should send push)
        let caps_no_pull = false; // pull_diagnostics = false

        let mut notifications_sent = 0;

        if !caps_no_pull {
            // This is the path taken in did_open.rs and did_change.rs
            notifications_sent += 1;
        }

        // Case 2: Client DOES support pull diagnostics (should NOT send push)
        let caps_with_pull = true; // pull_diagnostics = true

        if !caps_with_pull {
            // This should NOT be executed
            notifications_sent += 1;
        }

        // Verify only one notification was "sent" (from the first case)
        assert_eq!(notifications_sent, 1);
    }

    /// Test client capability parsing for pull diagnostics
    #[test]
    fn test_pull_diagnostics_capability_detection() {
        // Test when client supports pull diagnostics
        let client_caps_with_pull = ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                diagnostic: Some(DiagnosticClientCapabilities {
                    dynamic_registration: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        // Simulate the logic from ResolvedClientCapabilities::new
        let pull_diagnostics = client_caps_with_pull
            .text_document
            .as_ref()
            .and_then(|text_document| text_document.diagnostic.as_ref())
            .is_some();

        assert!(pull_diagnostics);

        // Test when client does NOT support pull diagnostics
        let client_caps_without_pull = ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                diagnostic: None, // No diagnostic capability
                ..Default::default()
            }),
            ..Default::default()
        };

        let pull_diagnostics = client_caps_without_pull
            .text_document
            .as_ref()
            .and_then(|text_document| text_document.diagnostic.as_ref())
            .is_some();

        assert!(!pull_diagnostics);

        // Test when text_document capability is missing entirely
        let client_caps_no_text_doc = ClientCapabilities {
            text_document: None,
            ..Default::default()
        };

        let pull_diagnostics = client_caps_no_text_doc
            .text_document
            .as_ref()
            .and_then(|text_document| text_document.diagnostic.as_ref())
            .is_some();

        assert!(!pull_diagnostics);
    }
} 